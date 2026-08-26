//! The vision nodes: what a graph can do with a frame.
//!
//! Sixteen nodes, grouped by what each one looks at:
//!
//! - [`detect`] — finding things in a frame, and taking apart what was found;
//! - [`faces`] — crop, embed, compare, remember;
//! - [`motion`] — whether anything moved, cheaply enough to be worth asking first;
//! - [`draw`] — boxes, labels, and hiding what should not be seen;
//! - [`raster`] — the pixel arithmetic underneath, no nodes.
//!
//! One thing runs through all of them: the frame reached this process from the edge, which is the
//! only component that touches a camera. Nothing here opens an RTSP session or holds a credential.

use super::{PortValue, PortValues};
use serde_json::Value;

pub(super) mod detect;
pub(super) mod draw;
pub(super) mod faces;
pub(super) mod motion;
pub(super) mod raster;

pub(super) use detect::*;
pub(super) use draw::*;
pub(super) use faces::*;
pub(super) use motion::*;
pub(super) use raster::*;

/// Embedded monospace font for the Draw Text node (CascadiaMono).
const FONT_BYTES: &[u8] = include_bytes!("../../../../assets/fonts/CascadiaMono-Regular.ttf");

/// Encode an RGB image to JPEG bytes.
pub(super) fn encode_jpeg(rgb: image::RgbImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Jpeg,
        )
        .map_err(|e| format!("could not encode image: {e}"))?;
    Ok(out)
}
