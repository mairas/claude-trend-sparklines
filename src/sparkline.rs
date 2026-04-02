use image::{ImageEncoder, Rgba, RgbaImage};

/// A single history data point with its window identity.
#[derive(Debug, Clone)]
pub struct HistoryPoint {
    pub ts: u64,
    pub pct: f64,
    pub resets_at: u64,
}

/// Parameters controlling the bitmap render.
#[derive(Debug, Clone)]
pub struct RenderParams {
    pub width_px: u32,
    pub height_px: u32,
    pub window_duration_secs: u64,
    pub now: u64,
    pub current_pct: f64,
    pub current_resets_at: u64,
    pub grid_interval_secs: u64,
}

const COLOR_GREEN: Rgba<u8> = Rgba([0, 200, 0, 180]);
const COLOR_RED: Rgba<u8> = Rgba([230, 0, 0, 180]);
const COLOR_GRAY_FILL: Rgba<u8> = Rgba([128, 128, 128, 100]);
const COLOR_PACE_LINE: Rgba<u8> = Rgba([200, 200, 200, 160]);
const COLOR_GRIDLINE: Rgba<u8> = Rgba([80, 80, 80, 60]);
const COLOR_RESET_MARKER: Rgba<u8> = Rgba([220, 200, 0, 200]);
const COLOR_TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);

/// Map a percentage (0-100) to a y pixel coordinate.
/// 0% is at the bottom (y = height-1), 100% is at the top (y = 0).
fn pct_to_y(pct: f64, height: u32) -> f64 {
    (height - 1) as f64 - (pct / 100.0 * (height - 1) as f64)
}

/// Interpolate usage percentage at a given timestamp from sorted history entries.
/// Returns None if the timestamp is before the first entry.
fn interpolate_usage(entries: &[HistoryPoint], ts: f64) -> Option<f64> {
    if entries.is_empty() {
        return None;
    }
    if ts < entries[0].ts as f64 {
        return None;
    }
    if ts >= entries[entries.len() - 1].ts as f64 {
        return Some(entries[entries.len() - 1].pct);
    }

    // Find the two surrounding entries
    for i in 0..entries.len() - 1 {
        let a = &entries[i];
        let b = &entries[i + 1];
        if ts >= a.ts as f64 && ts <= b.ts as f64 {
            let span = (b.ts as f64 - a.ts as f64).max(1.0);
            let frac = (ts - a.ts as f64) / span;
            return Some(a.pct + (b.pct - a.pct) * frac);
        }
    }

    Some(entries[entries.len() - 1].pct)
}

/// Find the window segment (by resets_at) that applies at a given timestamp.
/// Returns (segment_start_ts, segment_resets_at) or None.
fn find_segment(entries: &[HistoryPoint], ts: f64, params: &RenderParams) -> (f64, u64) {
    // Walk entries to find the segment active at this timestamp.
    // A segment is defined by a contiguous run of entries with the same resets_at.
    // The segment that applies at `ts` is the last segment whose first entry is <= ts.
    if entries.is_empty() {
        let window_start = params.now as f64 - params.window_duration_secs as f64;
        return (window_start, params.current_resets_at);
    }

    let mut seg_start = entries[0].ts as f64;
    let mut seg_resets_at = entries[0].resets_at;

    for i in 1..entries.len() {
        if entries[i].resets_at != entries[i - 1].resets_at {
            // New segment starts
            if entries[i].ts as f64 > ts {
                // This new segment starts after our timestamp, so previous segment applies
                return (seg_start, seg_resets_at);
            }
            seg_start = entries[i].ts as f64;
            seg_resets_at = entries[i].resets_at;
        }
    }

    (seg_start, seg_resets_at)
}

/// Calculate the pace percentage at a given timestamp within a segment.
/// Pace is linear 0% at segment start to 100% at resets_at.
fn pace_at(ts: f64, segment_start: f64, resets_at: u64) -> f64 {
    let duration = (resets_at as f64 - segment_start).max(1.0);
    let elapsed = (ts - segment_start).max(0.0);
    (elapsed / duration * 100.0).clamp(0.0, 100.0)
}

/// Find timestamps where resets_at changes between adjacent entries.
fn find_reset_boundaries(entries: &[HistoryPoint]) -> Vec<f64> {
    let mut boundaries = Vec::new();
    for i in 1..entries.len() {
        if entries[i].resets_at != entries[i - 1].resets_at {
            // Place marker at the midpoint between the two entries
            boundaries.push((entries[i - 1].ts as f64 + entries[i].ts as f64) / 2.0);
        }
    }
    boundaries
}

/// Render a sparkline as an RGBA bitmap image.
pub fn render_bitmap(entries: &[HistoryPoint], params: &RenderParams) -> RgbaImage {
    let w = params.width_px;
    let h = params.height_px;
    let mut img = RgbaImage::from_pixel(w, h, COLOR_TRANSPARENT);

    let window_start = params.now as f64 - params.window_duration_secs as f64;
    let window_span = params.window_duration_secs as f64;

    let reset_boundaries = find_reset_boundaries(entries);

    for x in 0..w {
        let ts = window_start + (x as f64 / (w - 1).max(1) as f64) * window_span;

        // Determine segment for pace calculation
        let (seg_start, seg_resets_at) = find_segment(entries, ts, params);
        let pace = pace_at(ts, seg_start, seg_resets_at);
        let pace_y = pct_to_y(pace, h);

        // Interpolate usage at this column
        let usage = interpolate_usage(entries, ts);

        // Check if this column is a reset boundary
        let is_reset = reset_boundaries.iter().any(|&bt| {
            let bx = ((bt - window_start) / window_span * (w - 1) as f64).round() as i64;
            (bx - x as i64).abs() <= 0
        });

        // Check if this column is a grid line
        let is_gridline = if params.grid_interval_secs > 0 {
            let aligned = (ts / params.grid_interval_secs as f64).floor()
                * params.grid_interval_secs as f64;
            let grid_x = ((aligned - window_start) / window_span * (w - 1) as f64).round() as i64;
            grid_x == x as i64
        } else {
            false
        };

        match usage {
            Some(usage_pct) => {
                let usage_y = pct_to_y(usage_pct, h);
                let fill_top_y = usage_y.min(pace_y);

                // Fill from bottom up to max(usage, pace)
                for y in 0..h {
                    let yf = y as f64;

                    if is_reset {
                        // Reset marker: full-height yellow line
                        img.put_pixel(x, y, COLOR_RESET_MARKER);
                    } else if yf >= fill_top_y {
                        // Below the higher of usage/pace — fill area
                        if yf >= usage_y && usage_pct <= pace {
                            // Usage region, under pace
                            img.put_pixel(x, y, COLOR_GREEN);
                        } else if yf >= usage_y && usage_pct > pace {
                            // Usage region, over pace — the part up to pace is green,
                            // the part above pace is red
                            if yf <= pace_y {
                                // Above pace line (y is smaller = higher) — red zone
                                img.put_pixel(x, y, COLOR_RED);
                            } else {
                                // Below pace line — green zone
                                img.put_pixel(x, y, COLOR_GREEN);
                            }
                        } else if yf >= pace_y && yf > usage_y {
                            // Between usage top and pace line (pace is below usage visually)
                            // This is the gray fill below pace when pace < usage
                            img.put_pixel(x, y, COLOR_GRAY_FILL);
                        }

                        // Pace line overlay (thin, 1px)
                        if (yf - pace_y).abs() < 1.0 {
                            img.put_pixel(x, y, COLOR_PACE_LINE);
                        }
                    }

                    // Gridline (faint, through entire height where there's fill)
                    if is_gridline && yf >= fill_top_y && !is_reset {
                        img.put_pixel(x, y, COLOR_GRIDLINE);
                    }
                }
            }
            None => {
                // No data (before first entry or future): gray fill below pace
                for y in 0..h {
                    let yf = y as f64;

                    if is_reset {
                        img.put_pixel(x, y, COLOR_RESET_MARKER);
                    } else if yf >= pace_y {
                        img.put_pixel(x, y, COLOR_GRAY_FILL);
                        if (yf - pace_y).abs() < 1.0 {
                            img.put_pixel(x, y, COLOR_PACE_LINE);
                        }
                    }

                    if is_gridline && yf >= pace_y && !is_reset {
                        img.put_pixel(x, y, COLOR_GRIDLINE);
                    }
                }
            }
        }
    }

    img
}

/// Encode an RgbaImage as PNG bytes in memory.
pub fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .expect("PNG encoding failed");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(width: u32, height: u32) -> RenderParams {
        RenderParams {
            width_px: width,
            height_px: height,
            window_duration_secs: 5 * 3600, // 5 hours
            now: 100_000,
            current_pct: 50.0,
            current_resets_at: 100_000 + 2 * 3600, // resets 2h from now
            grid_interval_secs: 3600,
        }
    }

    #[test]
    fn generates_image_with_correct_dimensions() {
        let params = make_params(200, 40);
        let img = render_bitmap(&[], &params);
        assert_eq!(img.width(), 200);
        assert_eq!(img.height(), 40);
    }

    #[test]
    fn empty_history_renders_pace_only() {
        let params = make_params(100, 30);
        let img = render_bitmap(&[], &params);
        // With no history, the pace line and gray fill should produce
        // some non-transparent pixels.
        let non_transparent = img.pixels().filter(|p| p.0[3] > 0).count();
        assert!(
            non_transparent > 0,
            "empty history should still render pace line and gray fill"
        );
    }

    #[test]
    fn usage_below_pace_is_green() {
        let now = 100_000u64;
        let window_dur = 5 * 3600u64;
        let resets_at = now + 2 * 3600;

        // Usage slowly ramping — well below linear pace
        let entries: Vec<HistoryPoint> = vec![
            HistoryPoint { ts: now - window_dur, pct: 0.0, resets_at },
            HistoryPoint { ts: now - window_dur / 2, pct: 10.0, resets_at },
            HistoryPoint { ts: now, pct: 20.0, resets_at },
        ];

        let params = RenderParams {
            width_px: 100,
            height_px: 30,
            window_duration_secs: window_dur,
            now,
            current_pct: 20.0,
            current_resets_at: resets_at,
            grid_interval_secs: 3600,
        };

        let img = render_bitmap(&entries, &params);

        // Count green-dominant pixels (G channel highest, alpha > 0)
        let green_pixels = img
            .pixels()
            .filter(|p| p.0[3] > 0 && p.0[1] > p.0[0] && p.0[1] > p.0[2])
            .count();
        assert!(
            green_pixels > 50,
            "usage below pace should produce green-filled area, got {green_pixels}"
        );
    }

    #[test]
    fn usage_above_pace_is_red() {
        let now = 100_000u64;
        let window_dur = 5 * 3600u64;
        let resets_at = now + 1 * 3600; // only 1h left — tight window

        // Usage ramping fast — above linear pace
        let entries: Vec<HistoryPoint> = vec![
            HistoryPoint { ts: now - window_dur, pct: 0.0, resets_at },
            HistoryPoint { ts: now - window_dur / 2, pct: 70.0, resets_at },
            HistoryPoint { ts: now, pct: 95.0, resets_at },
        ];

        let params = RenderParams {
            width_px: 100,
            height_px: 30,
            window_duration_secs: window_dur,
            now,
            current_pct: 95.0,
            current_resets_at: resets_at,
            grid_interval_secs: 3600,
        };

        let img = render_bitmap(&entries, &params);

        let red_pixels = img
            .pixels()
            .filter(|p| p.0[3] > 0 && p.0[0] > p.0[1] && p.0[0] > p.0[2])
            .count();
        assert!(
            red_pixels > 20,
            "usage above pace should produce red-filled area, got {red_pixels}"
        );
    }

    #[test]
    fn reset_marker_creates_visible_line() {
        let now = 100_000u64;
        let window_dur = 5 * 3600u64;
        let reset1 = now - 1000; // first window resets near now
        let reset2 = now + 2 * 3600; // second window

        let entries: Vec<HistoryPoint> = vec![
            HistoryPoint { ts: now - window_dur, pct: 0.0, resets_at: reset1 },
            HistoryPoint { ts: now - window_dur / 2, pct: 50.0, resets_at: reset1 },
            // Window boundary — resets_at changes
            HistoryPoint { ts: now - window_dur / 2 + 1, pct: 0.0, resets_at: reset2 },
            HistoryPoint { ts: now, pct: 30.0, resets_at: reset2 },
        ];

        let params = RenderParams {
            width_px: 200,
            height_px: 40,
            window_duration_secs: window_dur,
            now,
            current_pct: 30.0,
            current_resets_at: reset2,
            grid_interval_secs: 3600,
        };

        let img = render_bitmap(&entries, &params);

        // Yellow pixels: R and G channels high, B channel low
        let yellow_pixels = img
            .pixels()
            .filter(|p| p.0[3] > 0 && p.0[0] > 150 && p.0[1] > 150 && p.0[2] < 100)
            .count();
        assert!(
            yellow_pixels > 5,
            "reset boundary should produce yellow marker pixels, got {yellow_pixels}"
        );
    }

    #[test]
    fn transparent_background() {
        let now = 100_000u64;
        let window_dur = 5 * 3600u64;
        let resets_at = now + 2 * 3600;

        let entries: Vec<HistoryPoint> = vec![
            HistoryPoint { ts: now - window_dur, pct: 0.0, resets_at },
            HistoryPoint { ts: now, pct: 30.0, resets_at },
        ];

        let params = RenderParams {
            width_px: 100,
            height_px: 30,
            window_duration_secs: window_dur,
            now,
            current_pct: 30.0,
            current_resets_at: resets_at,
            grid_interval_secs: 3600,
        };

        let img = render_bitmap(&entries, &params);

        // Top row should be mostly transparent (usage is 30%, pace line
        // at most ~100% at far right, so top-left corner should be clear)
        let top_left = img.get_pixel(0, 0);
        assert_eq!(
            top_left.0[3], 0,
            "background pixels above curve should be fully transparent"
        );
    }

    #[test]
    fn save_test_render() {
        let now = 100_000u64;
        let window_dur = 5 * 3600u64;
        let reset1 = now - 2 * 3600 + 500;
        let reset2 = now + 2 * 3600;

        let entries: Vec<HistoryPoint> = vec![
            HistoryPoint { ts: now - window_dur, pct: 0.0, resets_at: reset1 },
            HistoryPoint { ts: now - 4 * 3600, pct: 15.0, resets_at: reset1 },
            HistoryPoint { ts: now - 3 * 3600, pct: 35.0, resets_at: reset1 },
            HistoryPoint { ts: now - 2 * 3600, pct: 60.0, resets_at: reset1 },
            // Reset boundary
            HistoryPoint { ts: now - 2 * 3600 + 1, pct: 0.0, resets_at: reset2 },
            HistoryPoint { ts: now - 1 * 3600, pct: 25.0, resets_at: reset2 },
            HistoryPoint { ts: now, pct: 45.0, resets_at: reset2 },
        ];

        let params = RenderParams {
            width_px: 300,
            height_px: 60,
            window_duration_secs: window_dur,
            now,
            current_pct: 45.0,
            current_resets_at: reset2,
            grid_interval_secs: 3600,
        };

        let img = render_bitmap(&entries, &params);
        let png_bytes = encode_png(&img);

        std::fs::write("/tmp/sparkline-test.png", &png_bytes).expect("failed to write test PNG");

        // This test always passes — it's for visual inspection
        assert!(!png_bytes.is_empty(), "PNG output should not be empty");
    }
}
