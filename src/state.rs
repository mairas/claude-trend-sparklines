use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Sessions not seen for this long are dropped from the state file.
const SESSION_TTL_SECS: u64 = 86_400;

/// Account-wide usage shared between sessions, and read by hooks, whose stdin
/// carries no `rate_limits`.
fn state_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".claude")
        .join("claude-trend-sparklines-state.json")
}

/// Record this session's `rate_limits` and return the freshest account figures.
/// Returns `None` when this render carries no `rate_limits`.
pub fn update(input_json: &Value, now: u64) -> Option<Value> {
    update_at(&state_path(), input_json, now)
}

// Each session's figures come from its own last API response, but it renders
// whenever anything changes in it, including typing into a session idle for
// hours. So the freshest figures are the ones that changed most recently, not
// the ones rendered last. A session's first report counts as unchanged: it may
// be stale too. A real mid-window reset still wins, because it shows up as a
// change in every active session.
fn update_at(path: &Path, input_json: &Value, now: u64) -> Option<Value> {
    let rate_limits = input_json
        .get("rate_limits")
        .filter(|r| r.as_object().is_some_and(|o| !o.is_empty()))?
        .clone();
    let session = input_json
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let mut sessions = read_sessions(path);
    sessions.retain(|_, s| field(s, "seen") + SESSION_TTL_SECS >= now);
    let changed = match sessions.get(&session) {
        Some(prev) if prev["rate_limits"] == rate_limits => field(prev, "changed"),
        Some(_) => now,
        None => 0,
    };
    sessions.insert(
        session,
        json!({ "rate_limits": rate_limits, "changed": changed, "seen": now }),
    );

    let resolved = sessions
        .values()
        .max_by_key(|s| (field(s, "changed"), field(s, "seen")))
        .map(|s| s["rate_limits"].clone())
        .unwrap_or(rate_limits);
    write_atomic(
        path,
        &json!({ "ts": now, "rate_limits": resolved, "sessions": sessions }),
    );
    Some(resolved)
}

fn field(session: &Value, name: &str) -> u64 {
    session[name].as_u64().unwrap_or(0)
}

fn read_sessions(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("sessions").and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

// Sessions render concurrently, so each writes its own temp file; the rename
// means a reader sees either the old state or the new one, never a partial
// write. Two sessions updating at once can still lose one session's entry; it
// comes back on that session's next render.
fn write_atomic(path: &Path, state: &Value) {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&tmp, state.to_string()).is_err() || std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn render(session: &str, seven_day: f64) -> Value {
        json!({
            "session_id": session,
            "rate_limits": {
                "five_hour": { "used_percentage": 1.0, "resets_at": 2000 },
                "seven_day": { "used_percentage": seven_day, "resets_at": 9000 }
            }
        })
    }

    fn seven_day(rl: &Value) -> f64 {
        rl["seven_day"]["used_percentage"].as_f64().unwrap()
    }

    fn read_state(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn setup() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");
        (dir, path)
    }

    #[test]
    fn first_render_resolves_to_its_own_figures() {
        let (_dir, path) = setup();
        let rl = update_at(&path, &render("a", 10.0), 1000).unwrap();
        assert_eq!(seven_day(&rl), 10.0);
    }

    #[test]
    fn hooks_read_resolved_figures_with_timestamp() {
        let (_dir, path) = setup();
        update_at(&path, &render("a", 10.0), 1000);
        let state = read_state(&path);
        assert_eq!(state["ts"], 1000);
        assert_eq!(seven_day(&state["rate_limits"]), 10.0);
    }

    #[test]
    fn stale_session_rerender_does_not_win() {
        let (_dir, path) = setup();
        update_at(&path, &render("idle", 70.0), 1000);
        update_at(&path, &render("active", 74.0), 1100);
        update_at(&path, &render("active", 75.0), 1200);
        // the idle session renders again with its old figures
        let rl = update_at(&path, &render("idle", 70.0), 1300).unwrap();
        assert_eq!(seven_day(&rl), 75.0);
        assert_eq!(seven_day(&read_state(&path)["rate_limits"]), 75.0);
    }

    #[test]
    fn new_session_first_report_does_not_win() {
        let (_dir, path) = setup();
        update_at(&path, &render("a", 69.0), 1000);
        update_at(&path, &render("a", 70.0), 1100);
        let rl = update_at(&path, &render("new", 0.0), 1200).unwrap();
        assert_eq!(seven_day(&rl), 70.0);
    }

    #[test]
    fn real_reset_wins_once_a_session_sees_it() {
        let (_dir, path) = setup();
        update_at(&path, &render("a", 94.0), 1000);
        update_at(&path, &render("a", 95.0), 1100);
        update_at(&path, &render("b", 95.0), 1150);
        let rl = update_at(&path, &render("a", 0.0), 1200).unwrap();
        assert_eq!(seven_day(&rl), 0.0);
        // b has not seen the reset yet and renders its unchanged figures
        let rl = update_at(&path, &render("b", 95.0), 1300).unwrap();
        assert_eq!(seven_day(&rl), 0.0);
    }

    #[test]
    fn render_without_rate_limits_changes_nothing() {
        let (_dir, path) = setup();
        update_at(&path, &render("a", 10.0), 1000);
        let before = fs::read_to_string(&path).unwrap();
        for input in [
            json!({ "session_id": "b" }),
            json!({ "session_id": "b", "rate_limits": null }),
            json!({ "session_id": "b", "rate_limits": {} }),
        ] {
            assert!(update_at(&path, &input, 1100).is_none());
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn sessions_unseen_for_a_day_are_dropped() {
        let (_dir, path) = setup();
        update_at(&path, &render("old", 10.0), 1000);
        update_at(&path, &render("a", 20.0), 1000 + SESSION_TTL_SECS + 1);
        let sessions = read_state(&path)["sessions"].as_object().unwrap().clone();
        assert_eq!(sessions.keys().collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn corrupt_state_file_is_replaced() {
        let (_dir, path) = setup();
        fs::write(&path, "{ not json").unwrap();
        let rl = update_at(&path, &render("a", 10.0), 1000).unwrap();
        assert_eq!(seven_day(&rl), 10.0);
        assert_eq!(seven_day(&read_state(&path)["rate_limits"]), 10.0);
    }

    #[test]
    fn leaves_only_the_state_file() {
        let (dir, path) = setup();
        update_at(&path, &render("a", 10.0), 1000);
        update_at(&path, &render("b", 11.0), 1100);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec!["state.json"]);
    }
}
