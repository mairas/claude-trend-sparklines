use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::SystemTime;

const WRITE_INTERVAL_SECS: u64 = 600; // 10 minutes
const MAX_ENTRIES: usize = 1500;
const KEEP_ENTRIES: usize = 1100;

/// Each JSONL line: our timestamp + the full stdin blob preserved verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub ts: u64,
    pub input: Value,
}

impl Entry {
    /// Extract a float from a JSON path like "rate_limits.five_hour.used_percentage"
    fn get_f64(&self, path: &[&str]) -> Option<f64> {
        let mut v = &self.input;
        for key in path {
            v = v.get(key)?;
        }
        v.as_f64()
    }

    /// Extract a u64 from a JSON path
    fn get_u64(&self, path: &[&str]) -> Option<u64> {
        let mut v = &self.input;
        for key in path {
            v = v.get(key)?;
        }
        v.as_u64()
    }

    pub fn five_hour_pct(&self) -> Option<f64> {
        self.get_f64(&["rate_limits", "five_hour", "used_percentage"])
    }

    pub fn seven_day_pct(&self) -> Option<f64> {
        self.get_f64(&["rate_limits", "seven_day", "used_percentage"])
    }

    pub fn five_hour_resets_at(&self) -> Option<u64> {
        self.get_u64(&["rate_limits", "five_hour", "resets_at"])
    }

    pub fn seven_day_resets_at(&self) -> Option<u64> {
        self.get_u64(&["rate_limits", "seven_day", "resets_at"])
    }
}

fn history_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".claude")
        .join("claude-trend-sparklines.jsonl")
}

/// Read all entries from the JSONL history file.
pub fn read() -> Vec<Entry> {
    read_from(&history_path())
}

fn read_from(path: &PathBuf) -> Vec<Entry> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<Entry>(&line) {
            entries.push(entry);
        }
        // Silently skip malformed lines (including legacy format entries)
    }
    entries
}

/// Append the raw stdin JSON as a new history entry.
pub fn maybe_append(input_json: &Value, now: u64) -> bool {
    maybe_append_to(&history_path(), input_json, now)
}

fn maybe_append_to(path: &PathBuf, input_json: &Value, now: u64) -> bool {
    if !should_write(path) {
        return false;
    }

    let entry = Entry {
        ts: now,
        input: input_json.clone(),
    };
    let Ok(line) = serde_json::to_string(&entry) else {
        return false;
    };
    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    if writeln!(file, "{line}").is_err() {
        return false;
    }
    drop(file);

    maybe_rotate(path);
    true
}

fn maybe_rotate(path: &PathBuf) {
    let entries = read_from(path);
    if entries.len() <= MAX_ENTRIES {
        return;
    }
    let keep = &entries[entries.len() - KEEP_ENTRIES..];
    let tmp = path.with_extension("tmp");
    let Ok(mut file) = fs::File::create(&tmp) else {
        return;
    };
    for entry in keep {
        if let Ok(line) = serde_json::to_string(entry) {
            let _ = writeln!(file, "{line}");
        }
    }
    let _ = fs::rename(&tmp, path);
}

fn should_write(path: &PathBuf) -> bool {
    let mtime = match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };
    let elapsed = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or_default()
        .as_secs();
    elapsed >= WRITE_INTERVAL_SECS
}

/// Filter entries to the current window using `resets_at` to identify window membership.
/// Falls back to timestamp-only filtering for legacy entries without resets_at.
pub fn window_entries(entries: &[Entry], window_start: u64, field: WindowField, current_resets_at: u64) -> Vec<(u64, f64)> {
    let mut result: Vec<(u64, f64)> = Vec::new();

    for entry in entries {
        if entry.ts < window_start {
            continue;
        }
        let Some(val) = field.get(entry) else {
            continue;
        };
        let entry_resets_at = field.resets_at(entry).unwrap_or(0);
        if current_resets_at > 0 && entry_resets_at > 0 && entry_resets_at != current_resets_at {
            continue;
        }
        result.push((entry.ts, val));
    }

    result
}

#[derive(Debug, Clone, Copy)]
pub enum WindowField {
    FiveHour,
    SevenDay,
}

impl WindowField {
    fn get(self, entry: &Entry) -> Option<f64> {
        match self {
            WindowField::FiveHour => entry.five_hour_pct(),
            WindowField::SevenDay => entry.seven_day_pct(),
        }
    }

    fn resets_at(self, entry: &Entry) -> Option<u64> {
        match self {
            WindowField::FiveHour => entry.five_hour_resets_at(),
            WindowField::SevenDay => entry.seven_day_resets_at(),
        }
    }
}

/// Interpolate a value at a specific timestamp from sorted (ts, value) pairs.
/// Returns `None` if the timestamp is before all entries (no data available).
pub fn interpolate_at(entries: &[(u64, f64)], ts: u64) -> Option<f64> {
    if entries.is_empty() {
        return None;
    }
    if ts < entries[0].0 {
        return None;
    }
    if ts >= entries[entries.len() - 1].0 {
        return Some(entries[entries.len() - 1].1);
    }
    for i in 0..entries.len() - 1 {
        let (t1, v1) = entries[i];
        let (t2, v2) = entries[i + 1];
        if t1 <= ts && ts <= t2 {
            if t1 == t2 {
                return Some(v1);
            }
            let frac = (ts - t1) as f64 / (t2 - t1) as f64;
            return Some(v1 + (v2 - v1) * frac);
        }
    }
    Some(entries[entries.len() - 1].1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::NamedTempFile;

    fn temp_path() -> PathBuf {
        let f = NamedTempFile::new().unwrap();
        f.into_temp_path().to_path_buf()
    }

    fn make_entry(ts: u64, fh: f64, sd: f64, fh_ra: u64, sd_ra: u64) -> Entry {
        Entry {
            ts,
            input: json!({
                "rate_limits": {
                    "five_hour": { "used_percentage": fh, "resets_at": fh_ra },
                    "seven_day": { "used_percentage": sd, "resets_at": sd_ra }
                }
            }),
        }
    }

    #[test]
    fn read_missing_file() {
        let entries = read_from(&PathBuf::from("/nonexistent/path.jsonl"));
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn append_and_read_roundtrip() {
        let path = temp_path();
        let input = json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 10.5, "resets_at": 2000 },
                "seven_day": { "used_percentage": 20.3, "resets_at": 9000 }
            },
            "model": { "display_name": "Opus 4.6" }
        });
        let entry = Entry { ts: 1000, input: input.clone() };
        let line = serde_json::to_string(&entry).unwrap();
        fs::write(&path, format!("{line}\n")).unwrap();

        let entries = read_from(&path);
        assert_eq!(entries.len(), 1);
        assert!((entries[0].five_hour_pct().unwrap() - 10.5).abs() < 0.01);
        assert_eq!(entries[0].five_hour_resets_at().unwrap(), 2000);
        // Verify extra fields are preserved
        assert_eq!(entries[0].input["model"]["display_name"], "Opus 4.6");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn skips_malformed_lines() {
        let path = temp_path();
        let entry = make_entry(1000, 10.0, 20.0, 0, 0);
        let good_line = serde_json::to_string(&entry).unwrap();
        fs::write(&path, format!("{good_line}\ngarbage line\n{good_line}\n")).unwrap();

        let entries = read_from(&path);
        assert_eq!(entries.len(), 2);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn interpolate_exact_match() {
        let entries = vec![(100, 10.0), (200, 20.0), (300, 30.0)];
        assert!((interpolate_at(&entries, 200).unwrap() - 20.0).abs() < 0.01);
    }

    #[test]
    fn interpolate_between() {
        let entries = vec![(100, 10.0), (200, 20.0)];
        assert!((interpolate_at(&entries, 150).unwrap() - 15.0).abs() < 0.01);
    }

    #[test]
    fn interpolate_before_first_returns_none() {
        let entries = vec![(100, 10.0), (200, 20.0)];
        assert!(interpolate_at(&entries, 50).is_none());
    }

    #[test]
    fn interpolate_after_last() {
        let entries = vec![(100, 10.0), (200, 20.0)];
        assert!((interpolate_at(&entries, 300).unwrap() - 20.0).abs() < 0.01);
    }

    #[test]
    fn interpolate_empty() {
        assert!(interpolate_at(&[], 100).is_none());
    }

    #[test]
    fn resets_at_filters_by_window() {
        let entries = vec![
            make_entry(100, 30.0, 40.0, 500, 9000),
            make_entry(200, 40.0, 41.0, 500, 9000),
            make_entry(300, 5.0, 42.0, 800, 9000),
            make_entry(400, 10.0, 42.5, 800, 9000),
        ];
        let result = window_entries(&entries, 0, WindowField::FiveHour, 800);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 300);
    }

    #[test]
    fn legacy_entries_without_resets_at_are_kept() {
        let entries = vec![
            make_entry(100, 10.0, 40.0, 0, 0),
            make_entry(200, 20.0, 41.0, 0, 0),
            make_entry(300, 30.0, 42.0, 0, 0),
        ];
        let result = window_entries(&entries, 0, WindowField::FiveHour, 500);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn window_entries_filters_before_start() {
        let entries = vec![
            make_entry(100, 10.0, 40.0, 500, 9000),
            make_entry(200, 20.0, 41.0, 500, 9000),
            make_entry(300, 30.0, 42.0, 500, 9000),
        ];
        let result = window_entries(&entries, 250, WindowField::FiveHour, 500);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 300);
    }

    #[test]
    fn rotation_keeps_recent() {
        let path = temp_path();
        {
            let mut file = fs::File::create(&path).unwrap();
            for i in 0..1510u64 {
                let entry = make_entry(i, i as f64, i as f64, 0, 0);
                let line = serde_json::to_string(&entry).unwrap();
                writeln!(file, "{line}").unwrap();
            }
        }
        assert!(read_from(&path).len() > MAX_ENTRIES);

        maybe_rotate(&path);

        let after = read_from(&path);
        assert_eq!(after.len(), KEEP_ENTRIES);
        assert_eq!(after[0].ts, 410);
        fs::remove_file(&path).ok();
    }
}
