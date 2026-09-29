# claude-trend-sparklines

A statusline for [Claude Code](https://claude.ai/code) that shows quota usage with inline sparkline trend graphs. Rust rewrite of [claude-pace](https://github.com/Astro-Han/claude-pace) with correct sparkline rendering.

```
Opus 4.6 (1M) ◑ | my-project (main) +12/-3 ~4
████░░░░░░ 42% 1M | 5h ▁▂▃▄▅▆▇█ 39% ⇡3% 3h  7d ▃▃▄▅▆▇█ 43% ⇣2% 2d
```

**⇡15%** = 15 points over pace, **⇣15%** = 15 points under. The color shows how hard the rest of the window has to be throttled, from the sustainable rate r = (100 − used%) / (100 − elapsed%): green when r ≥ 0.90, yellow ≥ 0.75, red below. The same overspend weighs more late in a window: 10 points over is r = 0.88 (yellow) at 15% elapsed and r = 0.75 at 60%.
Sparklines show cumulative usage vs. linear pace — green blocks are under pace, red blocks are over, gray blocks are the future pace reference.

## Why a rewrite?

The original bash script hit limitations with sparkline rendering:

- **Stale slot values** — bash used the last recorded history entry in each slot's time range, not the interpolated value at the slot boundary. Slots systematically understated usage.
- **Current slot rendered as gray** — live data was injected into the previous completed slot instead of the current one, causing red→green snapping on slot transitions.
- **Integer-only arithmetic** — interpolation, rounding, and boundary calculations required float math that bash couldn't provide cleanly.

The Rust version fixes all of these with proper interpolation, correct slot classification, and float arithmetic throughout. It also eliminates the `jq` dependency — a single binary with no runtime dependencies.

## Install

Build from source (requires Rust toolchain):

```bash
git clone https://github.com/mairas/claude-trend-sparklines.git
cd claude-trend-sparklines
cargo build --release
cp target/release/claude-trend-sparklines ~/.claude/
```

Add to `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/path/to/home/.claude/claude-trend-sparklines"
  }
}
```

Restart Claude Code.

### Usage in the model's context

The status line shows usage to you; the `inject` hook shows it to the model. Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "/path/to/home/.claude/claude-trend-sparklines inject" }] }
    ]
  }
}
```

On every prompt it adds a block like this:

```
<usage_limits scope="account, all sessions" as_of="2026-09-21 14:12Z" now="2026-09-21 14:13Z">
<limit window="5h" used="21%" resets="2026-09-21 16:42Z, in 2h 29m"/>
<limit window="7d" used="74%" r="0.52" resets="2026-09-25 01:13Z, in 3d 11h"/>
</usage_limits>
```

Times are UTC, with `now` given so the model can tell when a reset has passed. `as_of` is when a status line on this machine last wrote the figures; quota spent elsewhere since then, on another machine or in a headless run, is not in them. Used percentages are rounded down. r is reported for the 7d window only. A window past its reset is left out, and with no live window or no state file the hook adds nothing. It reads only the state file the status line writes, so it needs the status line installed.

### Gating subagents near the limit

The `gate` hook stops fan-out from spending the last of the quota:

| Condition | Decision |
|---|---|
| 5h ≥ 90% or 7d ≥ 95% | deny every subagent and workflow spawn until the window resets; the reason names the reset time and tells the model to continue inline |
| 7d r < 0.75 | ask the user before a workflow fan-out |
| otherwise | nothing |

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Agent|Workflow", "hooks": [{ "type": "command", "command": "/path/to/home/.claude/claude-trend-sparklines gate" }] }
    ]
  }
}
```

The gate decides each spawn afresh. While it denies, the `inject` block carries a `<gate subagents="denied" by="5h at 92%" until="…"/>` element, so the model can see on a later prompt that the element is gone and spawning works again. Install the two hooks together: the element only reports what the gate would decide, and the deny reason refers to the block.

## Features

- **Sparkline trend graphs** — 8-slot 5h window and 7-slot 7d window with interpolated boundary values
- **Pace delta** — compares usage to elapsed time (⇡ over / ⇣ under), colored by how much the rest of the window must be throttled
- **Effort level** — reads `effortLevel` from settings.json (●/◑/◔)
- **Git integration** — branch name and diff stats with 5-second cache
- **Worktree detection** — shows `repo/worktree` for Claude Code worktrees
- **Rich history** — JSONL log preserving full stdin context for future analysis
- **Window identity tracking** — uses `resets_at` timestamps instead of heuristic reset detection

## History

Usage data is logged to `~/.claude/claude-trend-sparklines.jsonl` every 10 minutes. Each line stores the full JSON context received from Claude Code, enabling future analysis beyond what the sparkline currently displays.

Rate limits are account-wide, but each session's figures come from its own last API response, and a session renders whenever anything changes in it, including typing into one that has been idle for hours. So every render records its session's figures in `~/.claude/claude-trend-sparklines-state.json`, and the status line and the history show the figures that changed most recently in any session, not the ones this session happens to hold. A session's first report counts as unchanged. A real mid-window reset still wins, because it changes every active session's figures. Claude Code gives rate limits only to the status line, so hooks read the resolved figures from the same file.

## Requirements

- Claude Code ≥ 2.1.80 (provides `rate_limits` in stdin)
- Rust toolchain (build only)

## License

MIT

## Acknowledgments

Inspired by and based on [claude-pace](https://github.com/Astro-Han/claude-pace) by [Astro-Han](https://github.com/Astro-Han).
