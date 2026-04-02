use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::RgbaImage;
use std::env;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GraphicsProtocol {
    Kitty,
    Sixel,
    None,
}

/// Detect graphics protocol from environment variables.
/// No terminal probing — avoids stdout contamination.
pub fn detect_protocol() -> GraphicsProtocol {
    let term = env::var("TERM").unwrap_or_default().to_lowercase();
    let term_program = env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();

    // Kitty protocol: Ghostty, Kitty, WezTerm, VS Code terminal
    if term.contains("ghostty")
        || term.contains("xterm-kitty")
        || term_program.contains("ghostty")
        || term_program.contains("kitty")
        || term_program.contains("wezterm")
    {
        return GraphicsProtocol::Kitty;
    }

    // Sixel: iTerm2, foot, others
    if term_program.contains("iterm") || term.contains("foot") {
        return GraphicsProtocol::Sixel;
    }

    GraphicsProtocol::None
}

#[derive(Clone, Copy)]
pub struct CellSize {
    pub width: u32,
    pub height: u32,
}

impl Default for CellSize {
    fn default() -> Self {
        Self {
            width: 8,
            height: 16,
        }
    }
}

pub fn query_cell_size() -> CellSize {
    CellSize::default()
}

/// Encode a PNG image as a Kitty graphics protocol escape sequence string.
/// The image occupies `cols` terminal columns.
pub fn kitty_inline(png_data: &[u8], cols: u32) -> String {
    let encoded = BASE64.encode(png_data);
    let chunk_size = 4096;
    let chunks: Vec<&str> = encoded
        .as_bytes()
        .chunks(chunk_size)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();

    let mut out = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = if i + 1 < chunks.len() { 1 } else { 0 };
        if i == 0 {
            write!(out, "\x1b_Gf=100,a=T,c={cols},m={more};{chunk}\x1b\\").ok();
        } else {
            write!(out, "\x1b_Gm={more};{chunk}\x1b\\").ok();
        }
    }
    out
}

/// Simple nearest-color palette for sixel output.
struct SixelPalette {
    colors: Vec<(u8, u8, u8)>,
}

impl SixelPalette {
    fn new() -> Self {
        Self {
            colors: vec![
                (0, 180, 0),     // 0: green
                (220, 50, 50),   // 1: red
                (140, 140, 140), // 2: gray
                (220, 180, 0),   // 3: yellow
            ],
        }
    }

    fn nearest(&self, r: u8, g: u8, b: u8) -> usize {
        self.colors
            .iter()
            .enumerate()
            .min_by_key(|(_, (cr, cg, cb))| {
                let dr = r as i32 - *cr as i32;
                let dg = g as i32 - *cg as i32;
                let db = b as i32 - *cb as i32;
                dr * dr + dg * dg + db * db
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn to_percent(v: u8) -> u32 {
        (v as u32 * 100 + 127) / 255
    }
}

/// Encode an RGBA image as a Sixel escape sequence string.
pub fn sixel_inline(img: &RgbaImage) -> String {
    let palette = SixelPalette::new();
    let (w, h) = img.dimensions();
    let mut out = String::new();

    write!(out, "\x1bPq").ok();

    for (i, (r, g, b)) in palette.colors.iter().enumerate() {
        let rp = SixelPalette::to_percent(*r);
        let gp = SixelPalette::to_percent(*g);
        let bp = SixelPalette::to_percent(*b);
        write!(out, "#{i};2;{rp};{gp};{bp}").ok();
    }

    let num_bands = (h + 5) / 6;
    for band in 0..num_bands {
        let y_start = band * 6;

        for color_idx in 0..palette.colors.len() {
            let mut has_color = false;
            'outer: for x in 0..w {
                for dy in 0..6 {
                    let y = y_start + dy;
                    if y < h {
                        let px = img.get_pixel(x, y);
                        if px[3] > 127 && palette.nearest(px[0], px[1], px[2]) == color_idx {
                            has_color = true;
                            break 'outer;
                        }
                    }
                }
            }
            if !has_color {
                continue;
            }

            write!(out, "#{color_idx}").ok();

            for x in 0..w {
                let mut sixel_bits: u8 = 0;
                for dy in 0..6 {
                    let y = y_start + dy;
                    if y < h {
                        let px = img.get_pixel(x, y);
                        if px[3] > 127
                            && palette.nearest(px[0], px[1], px[2]) == color_idx
                        {
                            sixel_bits |= 1 << dy;
                        }
                    }
                }
                write!(out, "{}", (sixel_bits + 63) as char).ok();
            }

            write!(out, "$").ok();
        }

        if band + 1 < num_bands {
            write!(out, "-").ok();
        }
    }

    write!(out, "\x1b\\").ok();
    out
}
