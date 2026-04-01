use crate::history;

/// ANSI color codes
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DARK_GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

/// Block elements indexed 0-8: space, then ▁▂▃▄▅▆▇█
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Classification of a sparkline slot.
#[derive(Debug, PartialEq)]
enum SlotKind {
    Completed,
    Current,
    Future,
}

/// Render a sparkline string for a usage window.
///
/// - `remaining_min`: minutes until the window resets
/// - `window_min`: total window duration in minutes (300 or 10080)
/// - `total_slots`: number of graph characters (8 or 7)
/// - `history_entries`: sorted (timestamp, cumulative_usage_pct) pairs, after reset detection
/// - `current_pct`: live usage percentage from stdin
/// - `now`: current unix timestamp
pub fn render(
    remaining_min: f64,
    window_min: f64,
    total_slots: usize,
    history_entries: &[(u64, f64)],
    current_pct: f64,
    now: u64,
) -> String {
    let elapsed_min = (window_min - remaining_min).max(0.0);
    let window_start = now as f64 - elapsed_min * 60.0;
    let window_secs = window_min * 60.0;

    // Determine which slot "now" falls into (0-indexed)
    let current_slot_idx = ((elapsed_min * total_slots as f64) / window_min)
        .floor()
        .min(total_slots as f64 - 1.0)
        .max(0.0) as usize;

    let mut out = String::new();

    for i in 0..total_slots {
        let slot_end_time = window_start + (i + 1) as f64 / total_slots as f64 * window_secs;
        let pace = (i + 1) as f64 / total_slots as f64 * 100.0;

        let kind = if i < current_slot_idx {
            SlotKind::Completed
        } else if i == current_slot_idx {
            SlotKind::Current
        } else {
            SlotKind::Future
        };

        match kind {
            SlotKind::Completed => {
                match history::interpolate_at(history_entries, slot_end_time as u64) {
                    Some(val) => {
                        let color = if val <= pace { GREEN } else { RED };
                        let block = value_to_block(val, total_slots);
                        out.push_str(&format!("{color}{block}{RESET}"));
                    }
                    None => {
                        // No data for this slot — render as gray pace reference
                        let block = value_to_block(pace, total_slots);
                        out.push_str(&format!("{DARK_GRAY}{block}{RESET}"));
                    }
                }
            }
            SlotKind::Current => {
                let val = current_pct;
                let color = if val <= pace { GREEN } else { RED };
                let block = value_to_block(val, total_slots);
                out.push_str(&format!("{color}{block}{RESET}"));
            }
            SlotKind::Future => {
                let block = value_to_block(pace, total_slots);
                out.push_str(&format!("{DARK_GRAY}{block}{RESET}"));
            }
        }
    }

    out
}

/// Map a percentage (0-100) to a block character.
/// Uses total_slots as the scale (so 100% maps to the highest block).
/// Minimum level for any positive value is 1 (▁).
fn value_to_block(pct: f64, total_slots: usize) -> char {
    let level = (pct * total_slots as f64 / 100.0)
        .round()
        .min(total_slots as f64)
        .max(0.0) as usize;
    // Ensure minimum ▁ for any non-zero value
    let level = if pct > 0.0 && level < 1 { 1 } else { level };
    // Clamp to available block chars
    let level = level.min(BLOCKS.len() - 1);
    BLOCKS[level]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn mostly_future_at_start() {
        // Window just started, no elapsed time
        let result = render(300.0, 300.0, 8, &[], 0.0, 1000);
        let plain = strip_ansi(&result);
        assert_eq!(plain.chars().count(), 8);
        // Slot 0 is current (green, 0% usage ≤ 12.5% pace), slots 1-7 are future (dark gray)
        assert!(result.contains(DARK_GRAY));
        assert!(result.contains(GREEN));
    }

    #[test]
    fn current_slot_shows_live_data() {
        // 37.5 min into 5h window = slot 0 is current
        let now = 1000 + 37 * 60; // ~37 min elapsed
        let result = render(263.0, 300.0, 8, &[], 5.0, now as u64);
        let plain = strip_ansi(&result);
        // First char should be ▁ (current slot with 5%), rest future
        assert_eq!(plain.chars().next().unwrap(), '▁');
    }

    #[test]
    fn completed_slot_uses_interpolation() {
        // 80 min into 5h window: slots 0,1 completed, slot 2 current
        let now: u64 = 10000;
        let window_start = now - 80 * 60;

        // History: steady ramp
        let entries: Vec<(u64, f64)> = vec![
            (window_start, 0.0),
            (window_start + 20 * 60, 8.0),  // 20 min in
            (window_start + 40 * 60, 16.0), // 40 min in (past slot 0 boundary at 37.5)
            (window_start + 60 * 60, 24.0), // 60 min in
            (window_start + 75 * 60, 30.0), // 75 min in (past slot 1 boundary at 75)
        ];

        let result = render(220.0, 300.0, 8, &entries, 32.0, now);
        let plain = strip_ansi(&result);
        assert_eq!(plain.chars().count(), 8);

        // Slot 0 boundary at 37.5 min: interpolated between (20min,8%) and (40min,16%)
        // = 8 + (16-8) * (17.5/20) = 8 + 7 = 15%
        // Pace at slot 0 = 12.5%. 15% > 12.5% → should be RED
        assert!(result.starts_with(RED));
    }

    #[test]
    fn under_pace_is_green() {
        let now: u64 = 10000;
        let window_start = now - 80 * 60;

        // Low usage: well under pace
        let entries: Vec<(u64, f64)> = vec![
            (window_start, 0.0),
            (window_start + 40 * 60, 5.0),
            (window_start + 75 * 60, 10.0),
        ];

        let result = render(220.0, 300.0, 8, &entries, 12.0, now);
        // Slot 0 boundary: interpolated ~5% at 37.5 min, pace 12.5% → green
        assert!(result.starts_with(GREEN));
    }

    #[test]
    fn future_slots_show_pace_reference() {
        // 37 min in, slot 0 is current, slots 1-7 are future
        let now: u64 = 10000;
        let result = render(263.0, 300.0, 8, &[], 5.0, now);
        let plain = strip_ansi(&result);
        let chars: Vec<char> = plain.chars().collect();
        // Future slots should show increasing pace blocks
        // Slot 1: pace 25% → block ~2, slot 2: pace 37.5% → block ~3, etc.
        // Just verify they're non-empty and increasing
        assert!(chars.len() == 8);
        for i in 2..8 {
            assert!(chars[i] >= chars[i - 1], "future blocks should be non-decreasing");
        }
    }

    #[test]
    fn seven_day_window() {
        let now: u64 = 100000;
        let result = render(10080.0, 10080.0, 7, &[], 0.0, now);
        let plain = strip_ansi(&result);
        assert_eq!(plain.chars().count(), 7);
    }

    #[test]
    fn block_mapping() {
        assert_eq!(value_to_block(0.0, 8), ' ');
        assert_eq!(value_to_block(1.0, 8), '▁'); // minimum for positive
        assert_eq!(value_to_block(100.0, 8), '█');
        assert_eq!(value_to_block(50.0, 8), '▄');
    }

    #[test]
    fn no_snapping_on_slot_transition() {
        // Simulate the bug scenario: usage over pace, transitioning from slot 1 to slot 2
        let now: u64 = 10000;
        let elapsed_min = 80.0;
        let window_start = now - (elapsed_min as u64) * 60;

        // Usage ramp that's above pace at slot 0 boundary
        let entries: Vec<(u64, f64)> = vec![
            (window_start, 0.0),
            (window_start + 10 * 60, 5.0),
            (window_start + 20 * 60, 12.0),
            (window_start + 35 * 60, 18.0), // near slot 0 end
            (window_start + 50 * 60, 22.0),
            (window_start + 70 * 60, 28.0), // near slot 1 end
        ];

        // Render at 80 min (slot 2 is current)
        let r1 = render(220.0, 300.0, 8, &entries, 32.0, now);

        // Render 20 min later at 100 min (slot 2 is still current, approaching slot 3)
        let now2 = now + 20 * 60;
        let entries2: Vec<(u64, f64)> = {
            let mut e = entries.clone();
            e.push((now2 - 10 * 60, 36.0));
            e
        };
        let r2 = render(200.0, 300.0, 8, &entries2, 40.0, now2);

        // Slot 0 color should be the same in both renders
        // (the completed slot shouldn't change color as time passes)
        let slot0_color_r1 = if r1.starts_with(RED) { "red" } else { "green" };
        let slot0_color_r2 = if r2.starts_with(RED) { "red" } else { "green" };
        assert_eq!(
            slot0_color_r1, slot0_color_r2,
            "completed slot 0 color must not change between renders"
        );
    }

    #[test]
    fn end_of_window() {
        // Very end of window: all slots completed or current
        let now: u64 = 10000;
        let result = render(1.0, 300.0, 8, &[], 95.0, now);
        let plain = strip_ansi(&result);
        assert_eq!(plain.chars().count(), 8);
    }
}
