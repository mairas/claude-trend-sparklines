use crate::pace::Limit;
use crate::{format, gate, pace, state};
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

/// A `<usage_limits>` block with one element per live window, plus a `<gate>`
/// element while subagents are denied, or `None` when no window is live. `as_of`
/// is when a status line on this machine last wrote the figures: quota spent
/// elsewhere since then is not in them.
fn block(rate_limits: &Value, as_of: u64, now: u64) -> Option<String> {
    let windows = pace::live_windows(rate_limits, now);
    let mut limits: Vec<String> = windows
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
    if let Some(w) = gate::blocking(&windows) {
        limits.push(format!(
            "<gate subagents=\"denied\" by=\"{} at {}%\" until=\"{}, {}\"/>",
            w.limit.label(),
            w.used.floor(),
            format::utc(w.resets_at),
            format::relative(w.remaining_secs)
        ));
    }

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
    use crate::pace::fixture::{NOW, limits, window};

    #[test]
    fn reports_every_live_window_with_r_for_the_week_only() {
        // 7d: 30% used at 50% elapsed, r = 1.40
        assert_eq!(
            block(&limits((20.0, 50.0), (30.0, 50.0)), NOW - 3600, NOW).unwrap(),
            "<usage_limits scope=\"account, all sessions\" as_of=\"2026-09-21 13:13Z\" now=\"2026-09-21 14:13Z\">\n\
             <limit window=\"5h\" used=\"20%\" resets=\"2026-09-21 16:43Z, in 2h 30m\"/>\n\
             <limit window=\"7d\" used=\"30%\" r=\"1.40\" resets=\"2026-09-25 02:13Z, in 3d 12h\"/>\n\
             </usage_limits>"
        );
    }

    #[test]
    fn used_rounds_down() {
        let text = block(&limits((99.6, 50.0), (30.0, 50.0)), NOW, NOW).unwrap();
        assert!(text.contains("window=\"5h\" used=\"99%\""), "{text}");
    }

    #[test]
    fn shows_the_gate_while_subagents_are_denied() {
        let text = block(&limits((92.0, 60.0), (40.0, 50.0)), NOW, NOW).unwrap();
        assert!(
            text.contains(
                "\n<gate subagents=\"denied\" by=\"5h at 92%\" until=\"2026-09-21 16:13Z, in 2h 0m\"/>\n</usage_limits>"
            ),
            "{text}"
        );
        let quiet = block(&limits((89.0, 60.0), (40.0, 50.0)), NOW, NOW).unwrap();
        assert!(!quiet.contains("<gate"), "{quiet}");
    }

    #[test]
    fn window_past_its_reset_is_left_out() {
        let expired = json!({ "used_percentage": 96.0, "resets_at": NOW - 60 });
        let rl = json!({
            "five_hour": window(20.0, 50.0, pace::FIVE_HOUR_MIN),
            "seven_day": expired,
        });
        let text = block(&rl, NOW, NOW).unwrap();
        assert!(text.contains("window=\"5h\""), "{text}");
        assert!(!text.contains("window=\"7d\""), "{text}");

        let rl = json!({ "five_hour": expired, "seven_day": expired });
        assert_eq!(block(&rl, NOW, NOW), None);
    }

    #[test]
    fn window_resetting_now_is_left_out() {
        let now = json!({ "used_percentage": 96.0, "resets_at": NOW });
        assert_eq!(
            block(&json!({ "five_hour": now, "seven_day": now }), NOW, NOW),
            None
        );
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
