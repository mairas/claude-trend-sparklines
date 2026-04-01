# claude-trend-sparklines

A statusline for [Claude Code](https://claude.ai/code) that shows quota usage with inline sparkline trend graphs. Rust rewrite of [claude-pace](https://github.com/Astro-Han/claude-pace) with correct sparkline rendering.

```
Opus 4.6 (1M) ◑ | my-project (main) +12/-3 ~4
████░░░░░░ 42% 1M | 5h ▁▂▃▄▅▆▇█ 39% ⇡3% 3h  7d ▃▃▄▅▆▇█ 43% ⇣2% 2d
```

**⇣15%** green = 15% under pace, you have headroom.
**⇡15%** red = 15% over pace, slow down.
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

## Features

- **Sparkline trend graphs** — 8-slot 5h window and 7-slot 7d window with interpolated boundary values
- **Pace delta** — compares usage rate to time remaining (⇡ over / ⇣ under)
- **Effort level** — reads `effortLevel` from settings.json (●/◑/◔)
- **Git integration** — branch name and diff stats with 5-second cache
- **Worktree detection** — shows `repo/worktree` for Claude Code worktrees
- **Rich history** — JSONL log preserving full stdin context for future analysis
- **Window identity tracking** — uses `resets_at` timestamps instead of heuristic reset detection

## History

Usage data is logged to `~/.claude/claude-trend-sparklines.jsonl` every 10 minutes. Each line stores the full JSON context received from Claude Code, enabling future analysis beyond what the sparkline currently displays.

## Requirements

- Claude Code ≥ 2.1.80 (provides `rate_limits` in stdin)
- Rust toolchain (build only)

## License

MIT

## Acknowledgments

Inspired by and based on [claude-pace](https://github.com/Astro-Han/claude-pace) by [Astro-Han](https://github.com/Astro-Han).
