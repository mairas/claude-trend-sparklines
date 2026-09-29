use crate::pace::Limit;
use crate::{format, pace, state};
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
    let limits: Vec<String> = pace::live_windows(rate_limits, now)
        .iter()
        .map(|w| {
            // r paces the week; the 5h window is read by its used % alone.
            let r = if w.limit == Limit::SevenDay {
                format!(" r=\"{:.2}\"", w.r)
            } else {
                String::new()
            };
            format!(
                "<limit window=\"{}\" used=\"{}%\"{r} resets=\"{}, {}\"/>",
                w.limit.label(),
                // Rounded down, so the figure reaches a threshold only when the usage does.
                w.used.floor(),
                format::utc(w.resets_at),
                format::relative(w.remaining_secs)
            )
        })
        .collect();

    (!limits.is_empty()).then(|| {
        format!(
            "<usage_limits scope=\"account, all sessions\" as_of=\"{}\" now=\"{}\">\n{}\n</usage_limits>",
            format::utc(as_of),
            format::utc(now),
            limits.join("\n")
        )
    })
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
}
