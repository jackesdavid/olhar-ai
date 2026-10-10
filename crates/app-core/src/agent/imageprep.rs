//! Frame → bounded base64 JPEG, shared by every LLM provider.
//!
//! Both providers need the same thing from a camera frame: a predictable media type (a snapshot
//! arrives as JPEG over HTTP or as a decoded frame from the RTSP fallback) and a payload small
//! enough to post. Keeping one copy means the providers cannot disagree about what the model saw.

use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use image::ImageFormat;

/// Cameras are 1080p+ and vision models downsize past ~1568px anyway, so cap the long edge to
/// keep the base64 payload modest without losing detail.
pub const MAX_EDGE: u32 = 1024;

/// Decode `raw` (any format the `image` crate reads), bound its long edge, and return it as
/// base64 JPEG. `who` names the caller so the error reaches the operator already attributed.
pub fn jpeg_b64(raw: &[u8], who: &str) -> Result<String, String> {
    let img =
        image::load_from_memory(raw).map_err(|e| format!("{who}: could not decode image: {e}"))?;
    let img = if img.width().max(img.height()) > MAX_EDGE {
        img.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut jpeg = Vec::new();
    img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .map_err(|e| format!("{who}: could not encode image: {e}"))?;
    Ok(B64.encode(&jpeg))
}
