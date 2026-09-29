use crate::pace::{self, Limit, Window};
use crate::{format, state};
use serde_json::{Value, json};

/// At or above these, subagents are denied until the window resets.
const DENY_5H_PCT: f64 = 90.0;
const DENY_7D_PCT: f64 = 95.0;
/// Below this 7d r, workflow fan-out asks the user first.
const ASK_7D_R: f64 = 0.75;

/// `PreToolUse` hook for `Agent` and `Workflow`: deny spawns near the quota
/// wall, and ask before a workflow fan-out when the week is far ahead of pace.
pub fn run(hook_input: &Value, now: u64) {
    if let Some(out) = response(hook_input, state::read().map(|(rl, _)| rl), now) {
        print!("{out}");
    }
}

/// The hook's output, or `None` to let the spawn through. No rate limits means
/// no decision.
fn response(hook_input: &Value, rate_limits: Option<Value>, now: u64) -> Option<Value> {
    let tool = hook_input
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (decision, reason) = decide(&rate_limits?, tool, now)?;
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": reason,
        }
    }))
}

/// The window holding subagents back, if any. When both are at their wall, the
/// one that resets last, since the gate holds until then.
pub fn blocking(windows: &[Window]) -> Option<&Window> {
    windows
        .iter()
        .filter(|w| {
            let wall = match w.limit {
                Limit::FiveHour => DENY_5H_PCT,
                Limit::SevenDay => DENY_7D_PCT,
            };
            w.used >= wall
        })
        .max_by_key(|w| w.remaining_secs)
}

fn decide(rate_limits: &Value, tool: &str, now: u64) -> Option<(&'static str, String)> {
    let windows = pace::live_windows(rate_limits, now);

    // The model has no clock, so the reason names an absolute time and says the
    // denial is per spawn; otherwise one deny reads as "no subagents this session".
    if let Some(w) = blocking(&windows) {
        return Some((
            "deny",
            format!(
                "Usage gate: {} at {}%, so subagents are denied until {} ({}). This spawn \
                 did not run; continue inline, and if it was a code review, run it inline and \
                 say that it ran inline. The gate checks every spawn, and the <usage_limits> \
                 block shows a <gate> element for as long as it applies.",
                w.limit.label(),
                w.used.floor(),
                format::utc(w.resets_at),
                format::relative(w.remaining_secs)
            ),
        ));
    }

    let week = windows.iter().find(|w| w.limit == Limit::SevenDay)?;
    (tool == "Workflow" && week.r < ASK_7D_R).then(|| {
        (
            "ask",
            format!(
                "Usage gate: 7d {:.0}% used, r {:.2}, resets {} ({}). The rest of the week \
                 can sustain {:.0}% of the nominal spend rate. Approve this workflow fan-out?",
                week.used,
                week.r,
                format::utc(week.resets_at),
                format::relative(week.remaining_secs),
                week.r * 100.0
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pace::fixture::{NOW, limits, window};

    fn decision(rl: &Value, tool: &str) -> Option<&'static str> {
        decide(rl, tool, NOW).map(|(d, _)| d)
    }

    #[test]
    fn quiet_when_nothing_is_near_a_limit() {
        let rl = limits((40.0, 50.0), (60.0, 50.0));
        assert_eq!(decision(&rl, "Agent"), None);
        assert_eq!(decision(&rl, "Workflow"), None);
    }

    #[test]
    fn five_hour_wall_denies_every_spawn() {
        let rl = limits((92.0, 60.0), (40.0, 50.0));
        for tool in ["Agent", "Workflow", ""] {
            let (decision, reason) = decide(&rl, tool, NOW).unwrap();
            assert_eq!(decision, "deny");
            assert!(reason.contains("5h at 92%"), "{reason}");
            assert!(
                reason.contains("until 2026-09-21 16:13Z (in 2h 0m)"),
                "{reason}"
            );
            assert!(reason.contains("continue inline"), "{reason}");
            assert!(reason.contains("code review"), "{reason}");
            assert!(reason.contains("checks every spawn"), "{reason}");
        }
    }

    #[test]
    fn five_hour_wall_is_inclusive() {
        assert_eq!(
            decision(&limits((90.0, 60.0), (40.0, 50.0)), "Agent"),
            Some("deny")
        );
        assert_eq!(decision(&limits((89.9, 60.0), (40.0, 50.0)), "Agent"), None);
    }

    #[test]
    fn seven_day_wall_denies() {
        let rl = limits((10.0, 50.0), (95.0, 90.0));
        let (decision, reason) = decide(&rl, "Agent", NOW).unwrap();
        assert_eq!(decision, "deny");
        assert!(reason.contains("7d at 95%"), "{reason}");
    }

    #[test]
    fn seven_day_between_the_two_walls_does_not_deny() {
        // 7d: 92% used at 90% elapsed, r = 0.80
        let rl = limits((10.0, 50.0), (92.0, 90.0));
        assert_eq!(decision(&rl, "Agent"), None);
        assert_eq!(decision(&rl, "Workflow"), None);
        let just_below = limits((10.0, 50.0), (94.9, 90.0));
        assert_eq!(decision(&just_below, "Agent"), None);
    }

    #[test]
    fn both_walls_name_the_later_reset() {
        let rl = limits((92.0, 60.0), (96.0, 90.0));
        let (_, reason) = decide(&rl, "Agent", NOW).unwrap();
        assert!(reason.contains("7d at 96%"), "{reason}");
        assert!(reason.contains("(in 16h 48m)"), "{reason}");
    }

    #[test]
    fn deny_wins_over_ask_for_a_workflow() {
        // 7d: 95% used at 90% elapsed, r = 0.50
        assert_eq!(
            decision(&limits((10.0, 50.0), (95.0, 90.0)), "Workflow"),
            Some("deny")
        );
    }

    #[test]
    fn week_far_ahead_of_pace_asks_before_a_workflow() {
        // 7d: 74% used at 50% elapsed, r = 0.52
        let rl = limits((10.0, 50.0), (74.0, 50.0));
        let (decision, reason) = decide(&rl, "Workflow", NOW).unwrap();
        assert_eq!(decision, "ask");
        assert!(reason.contains("r 0.52"), "{reason}");
        assert_eq!(decide(&rl, "Agent", NOW), None);
    }

    #[test]
    fn ask_threshold_is_exclusive() {
        // 7d at 50% elapsed: 62.55% used is r 0.749, 62.45% is r 0.751
        assert_eq!(
            decision(&limits((10.0, 50.0), (62.55, 50.0)), "Workflow"),
            Some("ask")
        );
        assert_eq!(
            decision(&limits((10.0, 50.0), (62.45, 50.0)), "Workflow"),
            None
        );
        // 62.5% at 50% elapsed is r = 0.75 exactly
        assert_eq!(
            decision(&limits((10.0, 50.0), (62.5, 50.0)), "Workflow"),
            None
        );
    }

    #[test]
    fn window_past_its_reset_does_not_deny() {
        let rl = json!({
            "five_hour": { "used_percentage": 99.0, "resets_at": NOW - 60 },
            "seven_day": window(40.0, 50.0, pace::SEVEN_DAY_MIN),
        });
        assert_eq!(decision(&rl, "Agent"), None);
    }

    #[test]
    fn no_rate_limits_lets_every_spawn_through() {
        let input = json!({ "tool_name": "Agent" });
        assert_eq!(response(&input, None, NOW), None);
    }

    #[test]
    fn response_reads_the_tool_and_wraps_the_decision() {
        let ahead = limits((10.0, 50.0), (74.0, 50.0));
        let out = response(
            &json!({ "tool_name": "Workflow" }),
            Some(ahead.clone()),
            NOW,
        )
        .unwrap();
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "ask");
        assert!(out["hookSpecificOutput"]["permissionDecisionReason"].is_string());
        assert_eq!(
            response(&json!({ "tool_name": "Agent" }), Some(ahead), NOW),
            None
        );

        let walled = limits((92.0, 60.0), (40.0, 50.0));
        let out = response(&json!({}), Some(walled), NOW).unwrap();
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    }
}
