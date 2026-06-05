//! Render a grayscale image into the cell grid as two-color phosphor *octants*:
//! each cell is a solid 2x4 block (U+1CD00, Unicode 16 "Symbols for Legacy
//! Computing Supplement") whose subpixels are split into a bright group (drawn
//! in the cell's foreground) and a dark group (its background). That resolves
//! the same 2x4 detail as Braille but, unlike Braille's single-tint 1-bit dots,
//! carries *two* independent phosphor shades per cell — so smooth camera
//! gradients survive without any dither pattern. Shared by the device-detail
//! panel and the chat snapshot modal.
//!
//! The expensive part — a Lanczos3 resize plus the per-cell split — is the
//! sharpest path, but it must NOT run on every redraw: the CRT window repaints
//! far more often than the source frame changes (and a snapshot never changes).
//! So [`CameraImage`] owns the source frame and a lazily-built cell grid, keyed
//! by the target cell dimensions. The grid is computed once per new frame (or
//! when the panel is resized) and simply blitted on every other redraw.
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use image::{imageops, GrayImage};
use ratatui::{layout::Rect, style::Color, Frame};

/// Octants (Unicode 16, 2024) aren't in most terminal fonts yet, but the CRT
/// window bundles a font that has them. Default to octants (the CRT frontend);
/// the plain-terminal frontend calls [`use_quadrant_glyphs`] to fall back to the
/// 2x2 quadrant set, which every terminal font has had for decades.
static OCTANT_MODE: AtomicBool = AtomicBool::new(true);

/// Switch image rendering to the universally-supported quadrant glyphs. Call
/// once at startup when running as a plain terminal app (`--tty`).
pub fn use_quadrant_glyphs() {
    OCTANT_MODE.store(false, Ordering::Relaxed);
}

/// Quadrant glyph for each of the 16 possible 2x2 fill patterns; bit `i` is the
/// subpixel at column `i%2`, row `i/2`. The terminal-mode fallback for octants.
const QUADRANTS: [char; 16] = [
    '\u{20}', '\u{2598}', '\u{259D}', '\u{2580}', '\u{2596}', '\u{258C}', '\u{259E}', '\u{259B}',
    '\u{2597}', '\u{259A}', '\u{2590}', '\u{259C}', '\u{2584}', '\u{2599}', '\u{259F}', '\u{2588}',
];

/// Octant glyph (U+1CD00 block, with Block-Elements fallbacks) for each of the
/// 256 possible 2x4 fill patterns. Bit `i` (value `1<<i`) is the subpixel at
/// column `i%2`, row `i/2` (row 0 = top). Generated from Unicode 16 names; the
/// handful of patterns without a dedicated codepoint snap to the nearest glyph.
const OCTANTS: [char; 256] = [
    '\u{20}', '\u{1CD04}', '\u{1CD01}', '\u{1CD02}', '\u{1CD00}', '\u{2598}', '\u{1CD01}', '\u{1CD02}',
    '\u{1CD03}', '\u{1CD04}', '\u{259D}', '\u{1CD05}', '\u{1CD06}', '\u{1CD07}', '\u{1CD08}', '\u{2580}',
    '\u{1CD09}', '\u{1CD0A}', '\u{1CD0B}', '\u{1CD0C}', '\u{1CD00}', '\u{1CD0D}', '\u{1CD0E}', '\u{1CD0F}',
    '\u{1CD10}', '\u{1CD11}', '\u{1CD12}', '\u{1CD13}', '\u{1CD14}', '\u{1CD15}', '\u{1CD16}', '\u{1CD17}',
    '\u{1CD18}', '\u{1CD19}', '\u{1CD1A}', '\u{1CD1B}', '\u{1CD1C}', '\u{1CD1D}', '\u{1CD1E}', '\u{1CD1F}',
    '\u{1CD03}', '\u{1CD20}', '\u{1CD21}', '\u{1CD22}', '\u{1CD23}', '\u{1CD24}', '\u{1CD25}', '\u{1CD26}',
    '\u{1CD27}', '\u{1CD28}', '\u{1CD29}', '\u{1CD2A}', '\u{1CD2B}', '\u{1CD2C}', '\u{1CD2D}', '\u{1CD2E}',
    '\u{1CD2F}', '\u{1CD30}', '\u{1CD31}', '\u{1CD32}', '\u{1CD33}', '\u{1CD34}', '\u{1CD35}', '\u{1CD17}',
    '\u{1CD36}', '\u{1CD36}', '\u{1CD37}', '\u{1CD38}', '\u{1CD39}', '\u{1CD3A}', '\u{1CD3B}', '\u{1CD3C}',
    '\u{1CD3D}', '\u{1CD3E}', '\u{1CD3F}', '\u{1CD40}', '\u{1CD41}', '\u{1CD42}', '\u{1CD43}', '\u{1CD44}',
    '\u{2596}', '\u{1CD45}', '\u{1CD46}', '\u{1CD47}', '\u{1CD48}', '\u{258C}', '\u{1CD49}', '\u{1CD4A}',
    '\u{1CD4B}', '\u{1CD4C}', '\u{259E}', '\u{1CD4D}', '\u{1CD4E}', '\u{1CD4F}', '\u{1CD50}', '\u{259B}',
    '\u{1CD51}', '\u{1CD52}', '\u{1CD53}', '\u{1CD54}', '\u{1CD55}', '\u{1CD56}', '\u{1CD57}', '\u{1CD58}',
    '\u{1CD59}', '\u{1CD5A}', '\u{1CD5B}', '\u{1CD5C}', '\u{1CD5D}', '\u{1CD5E}', '\u{1CD5F}', '\u{1CD60}',
    '\u{1CD61}', '\u{1CD62}', '\u{1CD63}', '\u{1CD64}', '\u{1CD65}', '\u{1CD66}', '\u{1CD67}', '\u{1CD68}',
    '\u{1CD69}', '\u{1CD6A}', '\u{1CD6B}', '\u{1CD6C}', '\u{1CD6D}', '\u{1CD6E}', '\u{1CD6F}', '\u{1CD70}',
    '\u{1CD71}', '\u{1CD71}', '\u{1CD72}', '\u{1CD73}', '\u{1CD74}', '\u{1CD75}', '\u{1CD76}', '\u{1CD77}',
    '\u{1CD78}', '\u{1CD79}', '\u{1CD7A}', '\u{1CD7B}', '\u{1CD7C}', '\u{1CD7D}', '\u{1CD7E}', '\u{1CD7F}',
    '\u{1CD80}', '\u{1CD81}', '\u{1CD82}', '\u{1CD83}', '\u{1CD84}', '\u{1CD85}', '\u{1CD86}', '\u{1CD87}',
    '\u{1CD88}', '\u{1CD89}', '\u{1CD8A}', '\u{1CD8B}', '\u{1CD8C}', '\u{1CD8D}', '\u{1CD8E}', '\u{1CD8F}',
    '\u{2597}', '\u{1CD90}', '\u{1CD91}', '\u{1CD92}', '\u{1CD93}', '\u{259A}', '\u{1CD94}', '\u{1CD95}',
    '\u{1CD96}', '\u{1CD97}', '\u{2590}', '\u{1CD98}', '\u{1CD99}', '\u{1CD9A}', '\u{1CD9B}', '\u{259C}',
    '\u{1CD9C}', '\u{1CD9D}', '\u{1CD9E}', '\u{1CD9F}', '\u{1CDA0}', '\u{1CDA1}', '\u{1CDA2}', '\u{1CDA3}',
    '\u{1CDA4}', '\u{1CDA5}', '\u{1CDA6}', '\u{1CDA7}', '\u{1CDA8}', '\u{1CDA9}', '\u{1CDAA}', '\u{1CDAB}',
    '\u{1CDAC}', '\u{1CDAC}', '\u{1CDAD}', '\u{1CDAE}', '\u{1CDAF}', '\u{1CDB0}', '\u{1CDB1}', '\u{1CDB2}',
    '\u{1CDB3}', '\u{1CDB4}', '\u{1CDB5}', '\u{1CDB6}', '\u{1CDB7}', '\u{1CDB8}', '\u{1CDB9}', '\u{1CDBA}',
    '\u{1CDBB}', '\u{1CDBC}', '\u{1CDBD}', '\u{1CDBE}', '\u{1CDBF}', '\u{1CDC0}', '\u{1CDC1}', '\u{1CDC2}',
    '\u{1CDC3}', '\u{1CDC4}', '\u{1CDC5}', '\u{1CDC6}', '\u{1CDC7}', '\u{1CDC8}', '\u{1CDC9}', '\u{1CDCA}',
    '\u{1CDCB}', '\u{1CDCC}', '\u{1CDCD}', '\u{1CDCE}', '\u{1CDCF}', '\u{1CDD0}', '\u{1CDD1}', '\u{1CDD2}',
    '\u{1CDD3}', '\u{1CDD4}', '\u{1CDD5}', '\u{1CDD6}', '\u{1CDD7}', '\u{1CDD8}', '\u{1CDD9}', '\u{1CDDA}',
    '\u{2584}', '\u{1CDDB}', '\u{1CDDC}', '\u{1CDDD}', '\u{1CDDE}', '\u{2599}', '\u{1CDDF}', '\u{1CDE0}',
    '\u{1CDE1}', '\u{1CDE2}', '\u{259F}', '\u{1CDE3}', '\u{1CD6D}', '\u{1CDE4}', '\u{1CDE5}', '\u{2588}',
];

/// A camera frame plus a cached, ready-to-blit cell grid. Wrap each new frame
/// (snapshot or live) in one of these; the resize+split cost is paid once and
/// reused until the target panel is resized.
pub struct CameraImage {
    img: GrayImage,
    cache: RefCell<Option<Grid>>,
}

/// The rendered cell grid for one specific target size. `cells` is row-major,
/// `w * h` entries of (glyph, foreground, background).
struct Grid {
    w: u16,
    h: u16,
    cells: Vec<(char, Color, Color)>,
}

impl CameraImage {
    pub fn new(img: GrayImage) -> Self {
        CameraImage { img, cache: RefCell::new(None) }
    }

    /// Blit the frame into `area`, (re)building the cached grid only if this is
    /// the first paint or the panel changed size.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        // CRT window: hand the raw frame to the GPU compositor, which draws it
        // as a full-resolution texture over these cells. Blank the cells so the
        // CRT renders black underneath the quad (the panel border, drawn in
        // surrounding cells, stays). The octant path below is the terminal-mode
        // fallback, where there's no GPU layer to composite into.
        if crate::ui::video::texture_mode() {
            crate::ui::video::submit(area, &self.img);
            let buf = f.buffer_mut();
            for cy in 0..area.height {
                for cx in 0..area.width {
                    if let Some(cell) = buf.cell_mut((area.x + cx, area.y + cy)) {
                        cell.set_char(' ');
                        cell.set_fg(Color::Black);
                        cell.set_bg(Color::Black);
                    }
                }
            }
            return;
        }

        let mut cache = self.cache.borrow_mut();
        let stale = !matches!(&*cache, Some(g) if g.w == area.width && g.h == area.height);
        if stale {
            *cache = Some(self.build(area.width, area.height));
        }
        let grid = cache.as_ref().unwrap();

        let buf = f.buffer_mut();
        let mut i = 0;
        for cy in 0..area.height {
            for cx in 0..area.width {
                let (ch, fg, bg) = grid.cells[i];
                i += 1;
                if let Some(cell) = buf.cell_mut((area.x + cx, area.y + cy)) {
                    cell.set_char(ch);
                    cell.set_fg(fg);
                    cell.set_bg(bg);
                }
            }
        }
    }

    /// Resize the source to the cell grid's subpixel resolution and split each
    /// cell into a bright/dark glyph (octant 2x4, or quadrant 2x2 in terminal
    /// mode). This is the costly path — called once per (frame, size), not once
    /// per redraw.
    fn build(&self, w: u16, h: u16) -> Grid {
        // Octants resolve 2x4 subpixels per cell; quadrants 2x2. Either way the
        // cell is 2 subpixels wide, so sample at 2x width and `sub_h`x height.
        let sub_h: u32 = if OCTANT_MODE.load(Ordering::Relaxed) { 4 } else { 2 };
        let glyph = |mask: usize| {
            if sub_h == 4 { OCTANTS[mask] } else { QUADRANTS[mask] }
        };
        let cols = w as u32 * 2;
        let rows = h as u32 * sub_h;
        // Lanczos3 gives noticeably sharper downscaling than bilinear; caching
        // means we only pay for it when the frame or panel size changes.
        let resized = imageops::resize(&self.img, cols, rows, imageops::FilterType::Lanczos3);

        let mut cells = Vec::with_capacity(w as usize * h as usize);
        for cy in 0..h {
            for cx in 0..w {
                // Gather the subpixel luminances (index = dy*2 + dx).
                let mut lum = [0u8; 8];
                let mut lo = 255u8;
                let mut hi = 0u8;
                for dy in 0..sub_h {
                    for dx in 0..2u32 {
                        let l = resized.get_pixel(cx as u32 * 2 + dx, cy as u32 * sub_h + dy)[0];
                        lum[(dy * 2 + dx) as usize] = l;
                        lo = lo.min(l);
                        hi = hi.max(l);
                    }
                }
                let count = (sub_h * 2) as usize;

                // Near-uniform cell: one flat shade, no glyph needed (the empty
                // pattern is a space, so the background color fills the cell).
                if hi - lo <= 6 {
                    let avg = ((lo as u16 + hi as u16) / 2) as u8;
                    let c = phosphor(avg);
                    cells.push((' ', c, c));
                    continue;
                }

                // Split subpixels at the cell midpoint into a bright group (the
                // glyph's foreground) and a dark group (its background), and tint
                // each by its own mean so gradients within the cell still read.
                let mid = ((lo as u16 + hi as u16) / 2) as u8;
                let mut mask = 0usize;
                let (mut hi_sum, mut hi_n, mut lo_sum, mut lo_n) = (0u32, 0u32, 0u32, 0u32);
                for (i, &l) in lum.iter().take(count).enumerate() {
                    if l > mid {
                        mask |= 1 << i;
                        hi_sum += l as u32;
                        hi_n += 1;
                    } else {
                        lo_sum += l as u32;
                        lo_n += 1;
                    }
                }
                let fg = phosphor((hi_sum / hi_n.max(1)) as u8);
                let bg = phosphor((lo_sum / lo_n.max(1)) as u8);
                cells.push((glyph(mask), fg, bg));
            }
        }
        Grid { w, h, cells }
    }
}

/// Map an 8-bit luminance to a phosphor-green shade.
fn phosphor(lum: u8) -> Color {
    Color::Rgb(0, lum, lum / 5)
}
