//! Bridge between the cell-grid view code and the CRT window's GPU video
//! compositor.
//!
//! The octant renderer ([`crate::ui::image`]) draws a camera frame into the
//! character grid — sharp text and sharp image are then locked to the same cell
//! size. To break that link in the CRT window, the camera frame is instead drawn
//! as a real GPU texture at full pixel resolution, *over* the rendered cells, by
//! a custom post-processor (`crate::crt`). This module is the hand-off point:
//! it's always compiled (no wgpu types here), holds the latest frame plus the
//! cell rectangle it belongs in, and is read by the GPU side each frame.
//!
//! When no compositor is installed (the `--tty` frontend), [`texture_mode`]
//! is false and the image renderer falls back to drawing octants as before.
use std::sync::{Mutex, OnceLock};

use image::GrayImage;
use ratatui::layout::Rect;

/// The shared frame buffer + placement the GPU compositor reads. One instance,
/// installed by the CRT frontend at startup.
#[derive(Default)]
pub struct Compositor {
    /// A camera frame was submitted during the current draw — the GPU side
    /// should composite it this pass. Reset to false by the frontend before
    /// each `terminal.draw`.
    pub active: bool,
    /// Terminal grid size in cells, written by the frontend each frame so the
    /// GPU side can map a cell rect to normalized device coordinates.
    pub cols: u16,
    pub rows: u16,
    /// The cell rectangle the frame occupies (x, y, w, h).
    pub rect: (u16, u16, u16, u16),
    /// Latest frame: dimensions and tightly-packed grayscale bytes.
    pub frame_w: u32,
    pub frame_h: u32,
    pub gray: Vec<u8>,
}

static COMPOSITOR: OnceLock<Mutex<Compositor>> = OnceLock::new();

/// Install the shared compositor (called once by the CRT frontend). After this,
/// [`texture_mode`] is true and the image renderer hands frames to the GPU.
#[cfg_attr(not(feature = "crt"), allow(dead_code))]
pub fn install() -> &'static Mutex<Compositor> {
    COMPOSITOR.get_or_init(|| Mutex::new(Compositor::default()))
}

/// Whether GPU texture compositing is active (CRT frontend running). When false
/// the image renderer draws octants into the cell grid as usual.
pub fn texture_mode() -> bool {
    COMPOSITOR.get().is_some()
}

/// Hand a camera frame and its cell rectangle to the GPU compositor for this
/// draw. No-op if no compositor is installed.
pub fn submit(area: Rect, img: &GrayImage) {
    let Some(lock) = COMPOSITOR.get() else { return };
    let Ok(mut c) = lock.lock() else { return };
    c.active = true;
    c.rect = (area.x, area.y, area.width, area.height);
    c.frame_w = img.width();
    c.frame_h = img.height();
    c.gray.clear();
    c.gray.extend_from_slice(img.as_raw());
}

/// Frontend hook: clear the per-draw `active` flag and record the current grid
/// size, just before rendering the view. Returns without effect if uninstalled.
#[cfg_attr(not(feature = "crt"), allow(dead_code))]
pub fn begin_draw(cols: u16, rows: u16) {
    if let Some(lock) = COMPOSITOR.get() {
        if let Ok(mut c) = lock.lock() {
            c.active = false;
            c.cols = cols.max(1);
            c.rows = rows.max(1);
        }
    }
}
