mod format;
mod git;
mod history;
mod input;
mod sparkline;
mod terminal;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let (input, raw_input) = input::Input::from_stdin();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // ── Effort level from settings.json ──
    let effort_icon = read_effort_icon();

    // ── Line 1: Model (Ctx) Effort | Project (branch) +a/-d ~f ──

    let model_label = input.model_label();
    let left1 = truncate_model_with_effort(&model_label, effort_icon, 22);

    let project_dir = input
        .workspace
        .as_ref()
        .and_then(|w| w.project_dir.as_deref())
        .unwrap_or(".");

    let (project_name, worktree_label) = detect_worktree(project_dir);
    let project_display = truncate_str(&project_name, 25);

    let git_info = git::info(project_dir);

    let mut right1 = if let Some(wt) = &worktree_label {
        // Worktree with detached HEAD: show repo/worktree
        if git_info.as_ref().is_some_and(|g| g.branch.is_empty()) {
            truncate_str(wt, 25)
        } else {
            project_display
        }
    } else {
        project_display
    };

    if let Some(ref gi) = git_info {
        if !gi.branch.is_empty() {
            let branch = truncate_str(&gi.branch, 35);
            right1 = format!("{right1} ({branch})");
        }
        if gi.is_dirty() {
            right1 = format!("{right1} {0}", gi.diff_label());
        }
    }

    // ── Line 2: bar pct% size | 5h [sparkline] pct% [delta] countdown  7d ... ──

    let ctx_pct = input.context_used_pct().unwrap_or(0.0);
    let ctx_size = input.context_size_label();

    let bar = format::progress_bar(ctx_pct);
    let ctx_label = format!("{} {}", format::color_pct(ctx_pct), ctx_size);
    let left2 = format!("{bar} {ctx_label}");

    // Rate limit windows
    let rl = input.rate_limits.as_ref();

    let window_5h = rl
        .and_then(|r| r.five_hour.as_ref())
        .and_then(|w| render_window(w, "5h", 300.0, 8, now));

    let window_7d = rl
        .and_then(|r| r.seven_day.as_ref())
        .and_then(|w| render_window(w, "7d", 10080.0, 7, now));

    let mut right2 = String::new();
    if let Some(w5) = window_5h {
        right2.push_str(&w5);
    }
    if let Some(w7) = window_7d {
        if !right2.is_empty() {
            right2.push_str("  ");
        }
        right2.push_str(&w7);
    }

    // Session cost: show when no rate limit data available
    if rl.is_none() {
        if let Some(cost) = input.cost.as_ref().and_then(|c| c.total_cost_usd) {
            if cost > 0.005 {
                right2.push_str(&format!("  ${cost:.2}"));
            }
        }
    }

    // ── History logging ──
    if !raw_input.is_null() {
        history::maybe_append(&raw_input, now);
    }

    // ── Pipe-aligned output ──
    let pipe = " | ";
    let left1_width = format::visible_width(&left1);
    let left2_width = format::visible_width(&left2);
    let pipe_col = left1_width.max(left2_width);

    let line1 = format!(
        "{}{pipe}{right1}",
        format::pad_to(&left1, pipe_col)
    );
    let line2 = format!(
        "{}{pipe}{right2}",
        format::pad_to(&left2, pipe_col)
    );

    println!("{line1}");
    print!("{line2}");
}

/// Read effort level from ~/.claude/settings.json and return the icon.
fn read_effort_icon() -> &'static str {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = Path::new(&home).join(".claude").join("settings.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return "◑",
    };
    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return "◑",
    };
    match val.get("effortLevel").and_then(|v| v.as_str()) {
        Some("high") => "●",
        Some("low") => "◔",
        _ => "◑",
    }
}

/// Truncate model name so that "model effort_icon" fits within max_width visible chars.
fn truncate_model_with_effort(model: &str, effort_icon: &str, max_width: usize) -> String {
    // "model effort" with a space between
    let total = model.chars().count() + 1 + effort_icon.chars().count();
    if total <= max_width {
        return format!("{model} {effort_icon}");
    }
    let avail = max_width.saturating_sub(2 + effort_icon.chars().count()); // 1 for space, 1 for "…"
    let truncated: String = model.chars().take(avail).collect();
    format!("{truncated}… {effort_icon}")
}

/// Detect if a path is a Claude Code worktree.
/// Returns (project_name, optional "repo/worktree" label).
fn detect_worktree(dir: &str) -> (String, Option<String>) {
    let basename = Path::new(dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string());

    // Pattern: .../<repo>/.claude/worktrees/<worktree-name>
    if let Some(idx) = dir.find("/.claude/worktrees/") {
        let repo = Path::new(&dir[..idx])
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let wt_name = &dir[idx + "/.claude/worktrees/".len()..];
        let wt_name = wt_name.split('/').next().unwrap_or(wt_name);
        return (repo.clone(), Some(format!("{repo}/{wt_name}")));
    }

    (basename, None)
}

/// Truncate a string to max visible chars, appending "…" if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

fn render_window(
    window: &input::WindowLimit,
    label: &str,
    window_min: f64,
    slots: usize,
    now: u64,
) -> Option<String> {
    let used_pct = window.used_percentage?;
    let resets_at = window.resets_at?;

    let remaining_secs = if resets_at > now {
        resets_at - now
    } else {
        0
    };
    let remaining_min = remaining_secs as f64 / 60.0;

    // Load history for sparkline
    let hist = history::read();
    let elapsed_min = (window_min - remaining_min).max(0.0);
    let window_start = now - (elapsed_min * 60.0) as u64;

    let field = match label {
        "5h" => history::WindowField::FiveHour,
        _ => history::WindowField::SevenDay,
    };
    let entries = history::window_entries(&hist, window_start, field, resets_at);

    let sl = sparkline::render(remaining_min, window_min, slots, &entries, used_pct, now);

    let pct_str = format::color_pct(used_pct);
    let delta = format::pace_delta(used_pct, remaining_min, window_min);
    let cd = format::countdown(remaining_min);

    Some(format!("{label} {sl} {pct_str}{delta} {cd}"))
}
