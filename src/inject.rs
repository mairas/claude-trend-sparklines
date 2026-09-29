use crate::{pace, state};
use serde_json::{Value, json};

/// `UserPromptSubmit` hook: add the account's usage to every prompt.
pub fn run(now: u64) {
    if let Some(text) = state::read().and_then(|(rl, as_of)| block(&rl, as_of, now)) {
        print!("{}", output(&text));
    }
}

fn output(text: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": text,
        }
    })
}

/// A `<usage_limits>` block with one element per live window, or `None` when no
/// window is live. `as_of` is when a status line on this machine last wrote the
/// figures: quota spent elsewhere since then is not in them.
fn block(rate_limits: &Value, as_of: u64, now: u64) -> Option<String> {
    let limits: Vec<String> = [
        ("5h", "five_hour", pace::FIVE_HOUR_MIN),
        ("7d", "seven_day", pace::SEVEN_DAY_MIN),
    ]
    .into_iter()
    .filter_map(|(label, key, window_min)| {
        let window = rate_limits.get(key)?;
        let used = window.get("used_percentage")?.as_f64()?;
        let resets_at = window.get("resets_at")?.as_u64()?;
        // A window past its reset holds last window's figures.
        let remaining_secs = resets_at.checked_sub(now).filter(|&s| s > 0)?;
        // r paces the week; the 5h window is read by its used % alone.
        let r = if key == "seven_day" {
            let r = pace::ratio(used, remaining_secs as f64 / 60.0, window_min);
            format!(" r=\"{r:.2}\"")
        } else {
            String::new()
        };
        Some(format!(
            "<limit window=\"{label}\" used=\"{}%\"{r} resets=\"{}, {}\"/>",
            // Rounded down, so the figure reaches a threshold only when the usage does.
            used.floor(),
            utc(resets_at),
            relative(remaining_secs)
        ))
    })
    .collect();

    (!limits.is_empty()).then(|| {
        format!(
            "<usage_limits scope=\"account, all sessions\" as_of=\"{}\" now=\"{}\">\n{}\n</usage_limits>",
            utc(as_of),
            utc(now),
            limits.join("\n")
        )
    })
}

fn relative(secs: u64) -> String {
    let (days, hours, minutes) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    if days > 0 {
        format!("in {days}d {hours}h")
    } else if hours > 0 {
        format!("in {hours}h {minutes}m")
    } else {
        format!("in {minutes}m")
    }
}

/// Unix seconds as `YYYY-MM-DD HH:MMZ`.
fn utc(secs: u64) -> String {
    // Civil-from-days, http://howardhinnant.github.io/date_algorithms.html
    let z = secs / 86_400 + 719_468;
    let (era, doe) = (z / 146_097, z % 146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    let (hour, minute) = (secs % 86_400 / 3600, secs % 3600 / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pace::{FIVE_HOUR_MIN, SEVEN_DAY_MIN};

    /// 2026-09-21 14:13Z
    const NOW: u64 = 1_790_000_000;

    /// A window `elapsed_pct` of the way through, with `used` percent spent.
    fn window(used: f64, elapsed_pct: f64, window_min: f64) -> Value {
        let remaining_secs = window_min * 60.0 * (100.0 - elapsed_pct) / 100.0;
        json!({ "used_percentage": used, "resets_at": NOW + remaining_secs as u64 })
    }

    fn limits(five_hour: Value, seven_day: Value) -> Value {
        json!({ "five_hour": five_hour, "seven_day": seven_day })
    }

    #[test]
    fn reports_every_live_window_with_r_for_the_week_only() {
        // 7d: 30% used at 50% elapsed, r = 1.40
        let rl = limits(
            window(20.0, 50.0, FIVE_HOUR_MIN),
            window(30.0, 50.0, SEVEN_DAY_MIN),
        );
        assert_eq!(
            block(&rl, NOW - 3600, NOW).unwrap(),
            "<usage_limits scope=\"account, all sessions\" as_of=\"2026-09-21 13:13Z\" now=\"2026-09-21 14:13Z\">\n\
             <limit window=\"5h\" used=\"20%\" resets=\"2026-09-21 16:43Z, in 2h 30m\"/>\n\
             <limit window=\"7d\" used=\"30%\" r=\"1.40\" resets=\"2026-09-25 02:13Z, in 3d 12h\"/>\n\
             </usage_limits>"
        );
    }

    #[test]
    fn used_rounds_down() {
        let rl = limits(
            window(99.6, 50.0, FIVE_HOUR_MIN),
            window(30.0, 50.0, SEVEN_DAY_MIN),
        );
        let text = block(&rl, NOW, NOW).unwrap();
        assert!(text.contains("window=\"5h\" used=\"99%\""), "{text}");
    }

    #[test]
    fn window_past_its_reset_is_left_out() {
        let expired = json!({ "used_percentage": 96.0, "resets_at": NOW - 60 });
        let rl = limits(window(20.0, 50.0, FIVE_HOUR_MIN), expired.clone());
        let text = block(&rl, NOW, NOW).unwrap();
        assert!(text.contains("window=\"5h\""), "{text}");
        assert!(!text.contains("window=\"7d\""), "{text}");

        assert_eq!(block(&limits(expired.clone(), expired), NOW, NOW), None);
    }

    #[test]
    fn window_resetting_now_is_left_out() {
        let now = json!({ "used_percentage": 96.0, "resets_at": NOW });
        assert_eq!(block(&limits(now.clone(), now), NOW, NOW), None);
    }

    #[test]
    fn missing_fields_are_quiet() {
        assert_eq!(block(&json!({}), NOW, NOW), None);
        assert_eq!(
            block(
                &json!({ "seven_day": { "used_percentage": 90.0 } }),
                NOW,
                NOW
            ),
            None
        );
    }

    #[test]
    fn hook_output_carries_the_block_as_prompt_context() {
        assert_eq!(
            output("<usage_limits/>"),
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": "<usage_limits/>",
                }
            })
        );
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative(12 * 60), "in 12m");
        assert_eq!(relative(100 * 60), "in 1h 40m");
        assert_eq!(relative((3 * 24 + 4) * 3600 + 59), "in 3d 4h");
        assert_eq!(relative(0), "in 0m");
    }

    #[test]
    fn utc_times() {
        assert_eq!(utc(0), "1970-01-01 00:00Z");
        assert_eq!(utc(NOW), "2026-09-21 14:13Z");
        // Last minute of a leap day
        assert_eq!(utc(951_782_400 + 86_399), "2000-02-29 23:59Z");
    }
}
