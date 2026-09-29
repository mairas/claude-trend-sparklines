use crate::pace;

/// ANSI escape codes
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const RED: &str = "\x1b[31m";
pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";

/// Color a usage percentage: green <70%, yellow 70-89%, red ≥90%.
pub fn color_pct(pct: f64) -> String {
    let color = pct_color(pct);
    format!("{color}{:.0}%{RESET}", pct)
}

pub fn pct_color(pct: f64) -> &'static str {
    if pct >= 90.0 {
        RED
    } else if pct >= 70.0 {
        YELLOW
    } else {
        GREEN
    }
}

/// Render a 10-block progress bar.
pub fn progress_bar(pct: f64) -> String {
    let filled = ((pct / 10.0).round() as usize).min(10);
    let empty = 10 - filled;
    let color = pct_color(pct);
    format!(
        "{color}{}{RESET}{}",
        "█".repeat(filled),
        "░".repeat(empty)
    )
}

/// Format a countdown in rounded units.
/// ≥ 1440 min → Xd, ≥ 60 min → Xh, otherwise Xm.
pub fn countdown(minutes: f64) -> String {
    if minutes >= 1440.0 {
        format!("{DIM}{}d{RESET}", (minutes / 1440.0).round() as u64)
    } else if minutes >= 60.0 {
        format!("{DIM}{}h{RESET}", (minutes / 60.0).round() as u64)
    } else {
        format!("{DIM}{}m{RESET}", minutes.round() as u64)
    }
}

/// Format pace delta with directional arrow: ⇡ over pace, ⇣ under, omitted when even.
/// The color follows the sustainable rate r, so the same delta reads worse late in a window.
pub fn pace_delta(used_pct: f64, remaining_min: f64, window_min: f64) -> String {
    let elapsed_pct = (window_min - remaining_min) / window_min * 100.0;
    let delta = (used_pct - elapsed_pct).round() as i64;
    let color = pace_color(pace::ratio(used_pct, remaining_min, window_min));
    if delta > 0 {
        format!(" {color}⇡{delta}%{RESET}")
    } else if delta < 0 {
        format!(" {color}⇣{}%{RESET}", delta.abs())
    } else {
        String::new()
    }
}

/// Color for the pace ratio r: green ≥0.9, yellow ≥0.75, red below.
fn pace_color(r: f64) -> &'static str {
    if r >= 0.9 {
        GREEN
    } else if r >= 0.75 {
        YELLOW
    } else {
        RED
    }
}

/// Measure visible (non-ANSI) character width of a string.
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            width += 1;
        }
    }
    width
}

/// Pad a string to a target visible width with spaces.
pub fn pad_to(s: &str, target_width: usize) -> String {
    let current = visible_width(s);
    if current >= target_width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(target_width - current))
    }
}

/// Format context window size as human-readable string.
pub fn context_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.0}M", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0}K", bytes as f64 / 1_000.0)
    } else if bytes > 0 {
        format!("{bytes}")
    } else {
        String::new()
    }
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
    fn progress_bar_empty() {
        let bar = strip_ansi(&progress_bar(0.0));
        assert_eq!(bar, "░░░░░░░░░░");
    }

    #[test]
    fn progress_bar_half() {
        let bar = strip_ansi(&progress_bar(50.0));
        assert_eq!(bar, "█████░░░░░");
    }

    #[test]
    fn progress_bar_full() {
        let bar = strip_ansi(&progress_bar(100.0));
        assert_eq!(bar, "██████████");
    }

    #[test]
    fn countdown_days() {
        let cd = strip_ansi(&countdown(10080.0));
        assert_eq!(cd, "7d");
    }

    #[test]
    fn countdown_hours_rounds() {
        let cd = strip_ansi(&countdown(300.0));
        assert_eq!(cd, "5h");
    }

    #[test]
    fn countdown_hours_rounds_up() {
        let cd = strip_ansi(&countdown(270.0));
        assert_eq!(cd, "5h"); // 4.5h rounds to 5h
    }

    #[test]
    fn countdown_minutes() {
        let cd = strip_ansi(&countdown(45.0));
        assert_eq!(cd, "45m");
    }

    #[test]
    fn pace_delta_over() {
        let pd = strip_ansi(&pace_delta(60.0, 150.0, 300.0));
        assert_eq!(pd, " ⇡10%"); // 60% used, 50% elapsed
    }

    #[test]
    fn pace_delta_under() {
        let pd = strip_ansi(&pace_delta(40.0, 150.0, 300.0));
        assert_eq!(pd, " ⇣10%"); // 40% used, 50% elapsed
    }

    #[test]
    fn pace_delta_even() {
        let pd = pace_delta(50.0, 150.0, 300.0);
        assert_eq!(pd, ""); // exactly on pace
    }

    fn remaining(elapsed_pct: f64) -> f64 {
        300.0 * (100.0 - elapsed_pct) / 100.0
    }

    #[test]
    fn pace_delta_color_follows_sustainable_rate() {
        // under pace, r = 1.2
        assert!(pace_delta(40.0, remaining(50.0), 300.0).starts_with(&format!(" {GREEN}")));
        // 2 points over early, r = 0.98: no throttling
        assert!(pace_delta(17.0, remaining(15.0), 300.0).starts_with(&format!(" {GREEN}")));
        // 10 points over early, r = 0.88
        assert!(pace_delta(25.0, remaining(15.0), 300.0).starts_with(&format!(" {YELLOW}")));
        // 10 points over at 50%, r = 0.80
        assert!(pace_delta(60.0, remaining(50.0), 300.0).starts_with(&format!(" {YELLOW}")));
        // 15 points over at 60%, r = 0.625
        assert!(pace_delta(75.0, remaining(60.0), 300.0).starts_with(&format!(" {RED}")));
    }

    #[test]
    fn visible_width_strips_ansi() {
        assert_eq!(visible_width("\x1b[32mhello\x1b[0m"), 5);
        assert_eq!(visible_width("hello"), 5);
    }

    #[test]
    fn pad_to_works() {
        let padded = pad_to("abc", 6);
        assert_eq!(padded, "abc   ");
    }

    #[test]
    fn pad_to_with_ansi() {
        let s = "\x1b[32mabc\x1b[0m";
        let padded = pad_to(s, 6);
        assert_eq!(visible_width(&padded), 6);
    }

    #[test]
    fn context_size_formatting() {
        assert_eq!(context_size(1_000_000), "1M");
        assert_eq!(context_size(200_000), "200K");
        assert_eq!(context_size(0), "");
    }
}
