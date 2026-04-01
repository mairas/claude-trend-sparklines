# Graphical Rolling Sparklines

## Problem

The current sparkline rendering uses Unicode block characters (▁▂▃▄▅▆▇█) which are limited to 8 discrete height levels per character cell. Modern terminals support inline pixel graphics via the Kitty and Sixel protocols, enabling smooth, detailed visualizations at full pixel resolution with alpha transparency.

## Solution

Replace character-block sparklines with pixel-rendered filled area charts displayed inline via terminal graphics protocols. Graphs become rolling time windows showing continuous data across window boundaries, with pace lines, reset markers, and gridlines.

## Visual Design

Each sparkline is a small inline image, transparent background, rendered at configurable width (default 12 characters for both 5h and 7d graphs) × 1 cell height.

**Elements rendered in each graph:**

1. **Usage curve** — Smooth filled area chart plotted from history data points. No slot bucketing — each data point is plotted at its actual time position.
   - Fill color: **green** (rgba with alpha) where usage ≤ pace at that x-position
   - Fill color: **red** (rgba with alpha) where usage > pace

2. **Pace line** — Thin line from 0% at window start to 100% at window end. When a reset occurs within the visible window, the pace line restarts from 0% at the reset point.

3. **Reset markers** — Vertical lines at positions where a window reset was detected (identified by change in `resets_at` value in history entries).

4. **Gridlines** — Faint vertical tick lines at:
   - Hour boundaries for the 5h graph
   - Day boundaries for the 7d graph

**Transparency:** All rendering uses RGBA with transparent background, so the graph composites cleanly over any terminal color scheme.

## Time Window Behavior

Each graph shows exactly one window period of time (5h or 7d), scrolling forward continuously.

- The right edge of the graph is "now"
- The left edge is "now minus window duration"
- When a reset occurs and scrolls within the visible window:
  - Previous window's remaining data appears on the left with its own pace line segment
  - Reset marker (vertical line) at the boundary
  - Current window's data on the right with pace line restarting from 0%
- As time passes, the reset marker scrolls off the left edge

## Data Pipeline

1. Read history entries from JSONL file
2. Filter to entries within the visible time range (now - window_duration, now)
3. Group by `resets_at` to identify window boundaries
4. For each pixel column:
   - Map x-position to timestamp
   - Interpolate usage value from surrounding history entries
   - Determine which window period this column belongs to
   - Calculate pace value relative to that window's start
   - Fill pixels: green below min(usage, pace), red between pace and usage (if over), transparent above usage

## Rendering Pipeline

1. **Query cell size** — Send `CSI 16 t` escape sequence to terminal, parse response for cell width/height in pixels. Fall back to 8×16 defaults if query fails or times out.
2. **Generate bitmap** — Create `image::RgbaImage` with width = graph_chars × cell_width, height = cell_height.
3. **Plot data** — For each pixel column, compute usage and pace values, fill pixels with appropriate RGBA colors.
4. **Display** — Pass `DynamicImage` to `viuer::print()` with inline positioning config.
5. **No graphics fallback** — If viuer detects no supported protocol (no Kitty, no Sixel), skip the sparkline entirely. The percentage, pace delta, and countdown are still shown.

## Configuration

Graph widths are configurable (mechanism TBD — settings.json or compile-time constants). Defaults:
- 5h graph: 12 characters wide
- 7d graph: 12 characters wide

## Dependencies

- `image` — bitmap generation (RgbaImage, pixel manipulation)
- `viuer` — terminal graphics protocol detection and image display

## Files Modified

- `src/sparkline.rs` — Replace character rendering with bitmap generation + viuer output
- `src/main.rs` — Cell size detection, pass pixel dimensions to sparkline renderer
- `Cargo.toml` — Add `image` and `viuer` dependencies

## Testing

- Unit tests for the data pipeline (interpolation, window grouping, pace calculation) remain unchanged
- Bitmap generation can be tested by writing output to PNG files and visually inspecting
- Integration testing requires a graphics-capable terminal

## Out of Scope

- Unicode block fallback (intentionally omitted — no sparkline if no graphics support)
- Cost or context overlays (may be added later)
- Interactive elements
