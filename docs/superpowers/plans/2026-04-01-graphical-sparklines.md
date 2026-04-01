# Graphical Rolling Sparklines Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace character-block sparklines with pixel-rendered filled area charts displayed inline via Kitty/Sixel terminal graphics protocols.

**Architecture:** Generate RGBA bitmaps with the `image` crate, detect terminal protocol support via `viuer`, and emit Kitty/Sixel escape sequences directly for inline display. The sparkline data pipeline changes from slot-based to continuous — each pixel column maps to a timestamp and interpolates usage from history.

**Tech Stack:** Rust, `image` crate (bitmap generation), `viuer` (protocol detection only), `base64` (Kitty protocol encoding)

**Spec:** `docs/superpowers/specs/2026-04-01-graphical-sparklines-design.md`

---

### Task 1: Add dependencies and create terminal module

**Files:**
- Modify: `Cargo.toml` — add `image` (png feature only), `viuer`, `base64`
- Create: `src/terminal.rs`
- Modify: `src/main.rs` — add `mod terminal`

**What to build:**
- `detect_protocol() -> GraphicsProtocol` — wraps viuer's `get_kitty_support()` and `is_sixel_supported()` to return a simple enum (Kitty / Sixel / None)
- `CellSize` struct with `query_cell_size()` — sends `CSI 16 t` to query terminal cell pixel dimensions, falls back to 8×16 defaults
- `emit_kitty_inline(png_data, cols)` — base64-encodes PNG data and emits via Kitty graphics protocol escape sequence (`\x1b_G...`), chunked at 4096 bytes, specifying column count for cursor advance
- `emit_sixel_inline(img)` — converts RGBA image to sixel encoding with a small fixed palette (green, red, gray) and emits inline

**Verification:** `cargo check` compiles cleanly.

- [ ] Implement terminal module
- [ ] Add mod declaration and dependencies
- [ ] Verify compilation
- [ ] Commit

---

### Task 2: Rewrite sparkline renderer for bitmap output

**Files:**
- Rewrite: `src/sparkline.rs`

**What to build:**

Replace the slot-based character renderer with a bitmap renderer. Key types:
- `HistoryPoint { ts, pct, resets_at }` — a single data point with window identity
- `RenderParams` — pixel dimensions, window duration, now, current_pct, current_resets_at, grid_interval_secs
- `render_bitmap(entries, params) -> RgbaImage` — the core rendering function
- `encode_png(img) -> Vec<u8>` — in-memory PNG encoding for Kitty protocol

**Rendering algorithm for each pixel column:**
1. Map x-position to timestamp within the visible window `[now - window_duration, now]`
2. Interpolate usage from surrounding history points (return None if before first entry)
3. Determine which window segment this column belongs to (using `resets_at` transitions in entries)
4. Calculate pace as linear 0→100% within that segment
5. Fill pixels bottom-to-top: green where usage ≤ pace, red where usage > pace, gray for future/no-data regions
6. Draw pace line (thin, light gray), gridlines (faint), and reset markers (vertical yellow lines)
7. Background is fully transparent (alpha=0)

**Tests to write:**
- Correct image dimensions
- Empty history renders pace reference only
- Usage below pace produces green pixels
- Usage above pace produces red pixels
- Reset markers produce visible lines at correct positions
- Background is transparent
- Save a representative render to PNG file for visual inspection

**Verification:** `cargo test` — all new tests pass.

- [ ] Write failing tests for bitmap renderer
- [ ] Implement `render_bitmap`, `HistoryPoint`, `RenderParams`, `encode_png`
- [ ] Verify tests pass
- [ ] Commit

---

### Task 3: Integrate bitmap sparklines into main output

**Files:**
- Modify: `src/main.rs` — update `render_window()` and `main()`
- Modify: `src/history.rs` — add `history_points()` extraction function

**What to build:**

- `history_points(entries, view_start, view_end, field) -> Vec<HistoryPoint>` in history.rs — extracts typed `HistoryPoint` data from raw history entries for a given window field and time range

- Update `main()` to detect protocol and query cell size at startup, pass them to `render_window()`

- Update `render_window()` signature to accept protocol, cell_size, and graph_cols (default 12). When protocol is not None: build `HistoryPoint` list, construct `RenderParams`, call `render_bitmap`, encode to PNG, emit via the appropriate protocol function. When protocol is None: skip sparkline, show only pct/delta/countdown.

- Grid interval: 3600s (hourly) for 5h graph, 86400s (daily) for 7d graph

**Verification:** `cargo test && cargo check` — existing tests still pass, compiles cleanly.

- [ ] Add `history_points()` to history.rs
- [ ] Update `render_window()` and `main()`
- [ ] Verify compilation and tests
- [ ] Commit

---

### Task 4: Build, deploy, and visually verify

- [ ] `cargo build --release`
- [ ] Test with synthetic JSON input in Ghostty — verify inline pixel sparklines display correctly
- [ ] Write a test that saves a representative render to `/tmp/sparkline-test.png` for visual inspection
- [ ] Deploy to `~/.claude/` and verify in live Claude Code session
- [ ] Commit any rendering fixes discovered during visual testing

---

### Task 5: Create upstream viuer issue

- [ ] Create issue in `mairas/claude-trend-sparklines` tracking the desire to contribute a `print_to_writer()` API upstream to viuer, so we can capture protocol output to a buffer instead of emitting escape sequences directly
