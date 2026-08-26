//! Pixel arithmetic, under everything above.
//!
//! No node lives here — only the primitives the drawing nodes are built from, plus the frame
//! comparison behind motion, which is the same kind of work: walking two buffers and counting.
//!
//! Kept apart because these are the pieces most likely to be read while chasing something that
//! looks wrong on screen, and they should be readable without the node logic around them.

use super::super::{device_from_camera, jpeg_dimensions, PortValue, PortValues};
use super::*;
use crate::agent::tools::ToolContext;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A detector's `[f32; 4]` xyxy box → the canonical integer `BBox`.
pub(super) fn bbox_from_xyxy(v: &[f32; 4]) -> crate::workflows::domain::BBox {
    crate::workflows::domain::BBox::from_xyxy(
        v[0].round() as i64,
        v[1].round() as i64,
        v[2].round() as i64,
        v[3].round() as i64,
    )
}

pub(super) fn motion_score(a: &[u8], b: &[u8], pixel_threshold: i16) -> Result<f64, String> {
    const W: u32 = 320;
    const H: u32 = 240;
    let gray = |raw: &[u8]| -> Result<Vec<u8>, String> {
        let img =
            image::load_from_memory(raw).map_err(|e| format!("Could not decode frame: {e}"))?;
        Ok(img
            .resize_exact(W, H, image::imageops::FilterType::Triangle)
            .to_luma8()
            .into_raw())
    };
    let ga = gray(a)?;
    let gb = gray(b)?;
    let total = ga.len().max(1);
    let changed = ga
        .iter()
        .zip(gb.iter())
        .filter(|(x, y)| (**x as i16 - **y as i16).abs() > pixel_threshold)
        .count();
    Ok((changed as f64 / total as f64) * 100.0)
}

pub(super) fn cfg_i64(config: &Value, k: &str, d: i64) -> i64 {
    config
        .get(k)
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(d)
}

pub(super) fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

/// Copy the pixels inside the rect from `src` over `dst` (clamped to bounds).
/// Empty when the rect lies fully outside the frame.
pub(super) fn copy_rect(
    src: &image::RgbImage,
    dst: &mut image::RgbImage,
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
) {
    let w = src.width() as i64;
    let h = src.height() as i64;
    let lo_x = x1.min(x2).max(0);
    let hi_x = x1.max(x2).min(w - 1);
    let lo_y = y1.min(y2).max(0);
    let hi_y = y1.max(y2).min(h - 1);
    for y in lo_y..=hi_y {
        for x in lo_x..=hi_x {
            let px = *src.get_pixel(x as u32, y as u32);
            dst.put_pixel(x as u32, y as u32, px);
        }
    }
}

pub(super) fn draw_rect(
    img: &mut image::RgbImage,
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
    color: [u8; 3],
    thickness: u32,
) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let px = image::Rgb(color);
    let t = thickness as i64;
    for d in 0..t {
        for x in x1..=x2 {
            if x >= 0 && x < w {
                if y1 + d >= 0 && y1 + d < h {
                    img.put_pixel(x as u32, (y1 + d) as u32, px);
                }
                if y2 - d >= 0 && y2 - d < h {
                    img.put_pixel(x as u32, (y2 - d) as u32, px);
                }
            }
        }
        for y in y1..=y2 {
            if y >= 0 && y < h {
                if x1 + d >= 0 && x1 + d < w {
                    img.put_pixel((x1 + d) as u32, y as u32, px);
                }
                if x2 - d >= 0 && x2 - d < w {
                    img.put_pixel((x2 - d) as u32, y as u32, px);
                }
            }
        }
    }
}

/// Fill an inclusive pixel block [ax..=bx, ay..=by], clamped to the frame.
/// No-op when the block is empty or entirely off-frame.
pub(super) fn fill_block(
    img: &mut image::RgbImage,
    ax: i64,
    ay: i64,
    bx: i64,
    by: i64,
    color: [u8; 3],
) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let px = image::Rgb(color);
    let lo_x = ax.min(bx).max(0);
    let hi_x = ax.max(bx).min(w - 1);
    let lo_y = ay.min(by).max(0);
    let hi_y = ay.max(by).min(h - 1);
    for y in lo_y..=hi_y {
        for x in lo_x..=hi_x {
            img.put_pixel(x as u32, y as u32, px);
        }
    }
}

/// HUD style: L-shaped brackets at each corner instead of a full rectangle.
pub(super) fn draw_corners(
    img: &mut image::RgbImage,
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
    color: [u8; 3],
    thickness: u32,
) {
    let t = thickness.max(1) as i64;
    // Arm length: a fraction of the shorter side, clamped to a sane range.
    let arm = ((x2 - x1).min(y2 - y1) / 4).clamp(6, 48);
    // Top-left
    fill_block(img, x1, y1, x1 + arm, y1 + t - 1, color);
    fill_block(img, x1, y1, x1 + t - 1, y1 + arm, color);
    // Top-right
    fill_block(img, x2 - arm, y1, x2, y1 + t - 1, color);
    fill_block(img, x2 - t + 1, y1, x2, y1 + arm, color);
    // Bottom-left
    fill_block(img, x1, y2 - t + 1, x1 + arm, y2, color);
    fill_block(img, x1, y2 - arm, x1 + t - 1, y2, color);
    // Bottom-right
    fill_block(img, x2 - arm, y2 - t + 1, x2, y2, color);
    fill_block(img, x2 - t + 1, y2 - arm, x2, y2, color);
}

/// HUD style: corner brackets + a centre crosshair with a small square (the
/// targeting-reticle look). Crosshair arms leave a gap around the centre square.
pub(super) fn draw_reticle(
    img: &mut image::RgbImage,
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
    color: [u8; 3],
    thickness: u32,
) {
    draw_corners(img, x1, y1, x2, y2, color, thickness);
    let t = thickness.max(1) as i64;
    let cx = (x1 + x2) / 2;
    let cy = (y1 + y2) / 2;
    let half = ((x2 - x1).min(y2 - y1) / 12).clamp(4, 16); // centre square half-size
                                                           // Small centre square outline.
    draw_rect(
        img,
        cx - half,
        cy - half,
        cx + half,
        cy + half,
        color,
        thickness,
    );
    // Crosshair arms (gap around the square so they don't touch it).
    let gap = half + 4;
    let len = (half * 2).max(8);
    fill_block(img, cx - gap - len, cy, cx - gap, cy + t - 1, color); // left
    fill_block(img, cx + gap, cy, cx + gap + len, cy + t - 1, color); // right
    fill_block(img, cx, cy - gap - len, cx + t - 1, cy - gap, color); // up
    fill_block(img, cx, cy + gap, cx + t - 1, cy + gap + len, color); // down
}
