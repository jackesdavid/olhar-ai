//! Drawing on a frame: boxes, labels, and hiding what should not be seen.
//!
//! These nodes exist so a graph can produce something a PERSON looks at — a snapshot in a
//! notification, a frame in the Console — rather than a list of coordinates.
//!
//! `BlurOutsideBbox` is the odd one out and the reason to be careful here: it is a privacy
//! control, not a decoration. What it hides must actually be gone from the bytes that leave, not
//! merely covered in a preview.

use super::super::{device_from_camera, jpeg_dimensions, PortValue, PortValues};
use super::*;
use crate::agent::tools::ToolContext;
use serde_json::Value;

pub(crate) fn run_get_image_size(inputs: &PortValues) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let (w, h) = jpeg_dimensions(bytes);
    let mut o = PortValues::new();
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(w > 0 && h > 0));
    Ok(o)
}

pub(crate) fn run_draw_bbox(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let raw = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let b = inputs
        .get("bbox")
        .and_then(PortValue::as_bbox)
        .ok_or("missing required input 'bbox'")?;
    let color_str = config
        .get("color")
        .and_then(|v| v.as_str())
        .unwrap_or("#00FF00");
    let color = parse_hex_color(color_str).unwrap_or([0, 255, 0]);
    let thickness = config
        .get("thickness")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<u32>().ok())
                .or_else(|| v.as_u64().map(|n| n as u32))
        })
        .unwrap_or(2)
        .max(1);
    let style = config
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("box");
    // Optional: grow the box outward by `expand`% of its size on each side (draw fns clamp to image).
    let expand = config
        .get("expand")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0)
        .max(0);
    let (mut x1, mut y1, mut x2, mut y2) = (b.x1, b.y1, b.x2, b.y2);
    if expand > 0 {
        let dx = (x2 - x1).max(0) * expand / 100;
        let dy = (y2 - y1).max(0) * expand / 100;
        x1 -= dx;
        y1 -= dy;
        x2 += dx;
        y2 += dy;
    }
    let img = image::load_from_memory(raw).map_err(|e| format!("could not decode image: {e}"))?;
    let mut rgb = img.into_rgb8();
    let (iw, ih) = (rgb.width() as i64, rgb.height() as i64);
    match style {
        "corners" => draw_corners(&mut rgb, x1, y1, x2, y2, color, thickness),
        "reticle" => draw_reticle(&mut rgb, x1, y1, x2, y2, color, thickness),
        _ => draw_rect(&mut rgb, x1, y1, x2, y2, color, thickness),
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Jpeg,
        )
        .map_err(|e| format!("could not encode image: {e}"))?;
    let w = out.len();
    log::debug!("draw_bbox: drew {thickness}px {style} [{x1},{y1},{x2},{y2}] on {w}-byte JPEG");
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(out));
    // The (possibly expanded) box we drew, clamped to the frame, so downstream nodes can reuse
    // the expanded region without off-frame coordinates.
    o.insert(
        "bbox".into(),
        PortValue::Bbox(crate::workflows::domain::BBox::from_xyxy(
            x1.clamp(0, iw),
            y1.clamp(0, ih),
            x2.clamp(0, iw),
            y2.clamp(0, ih),
        )),
    );
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Draw a text label anchored to a corner of `bbox`, optionally on a filled background rect.
pub(crate) fn run_draw_text(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    use ab_glyph::{FontRef, PxScale};
    use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size};
    use imageproc::rect::Rect;

    let raw = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let b = inputs
        .get("bbox")
        .and_then(PortValue::as_bbox)
        .ok_or("missing required input 'bbox'")?;
    // Text: wired input wins, else the config literal.
    let text = inputs
        .get("text")
        .and_then(PortValue::as_text)
        .map(str::to_string)
        .or_else(|| {
            config
                .get("text")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();

    let img = image::load_from_memory(raw).map_err(|e| format!("could not decode image: {e}"))?;
    let mut rgb = img.into_rgb8();
    if text.trim().is_empty() {
        // Nothing to draw — pass the image through unchanged.
        let out = encode_jpeg(rgb)?;
        let mut o = PortValues::new();
        o.insert("image".into(), PortValue::Image(out));
        o.insert("ok".into(), PortValue::Bool(false));
        return Ok(o);
    }
    let (iw, ih) = (rgb.width() as i64, rgb.height() as i64);

    let position = config
        .get("position")
        .and_then(|v| v.as_str())
        .unwrap_or("top-left");
    let size = config
        .get("size")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .or_else(|| v.as_f64().map(|n| n as f32))
        })
        .unwrap_or(18.0)
        .clamp(6.0, 200.0);
    let text_color = parse_hex_color(
        config
            .get("text_color")
            .and_then(|v| v.as_str())
            .unwrap_or("#FFFFFF"),
    )
    .unwrap_or([255, 255, 255]);
    let bg_on = config
        .get("background")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let bg_color = parse_hex_color(
        config
            .get("bg_color")
            .and_then(|v| v.as_str())
            .unwrap_or("#000000"),
    )
    .unwrap_or([0, 0, 0]);

    let font = FontRef::try_from_slice(FONT_BYTES).map_err(|e| format!("font load: {e}"))?;
    let scale = PxScale::from(size);
    let (tw, th) = text_size(scale, &font, &text);
    let (tw, th) = (tw as i64, th as i64);
    let pad: i64 = 4;

    // Anchor the text box to the chosen bbox corner (inside the box, with padding).
    let (tx, ty) = match position {
        "top-right" => (b.x2 - tw - pad, b.y1 + pad),
        "bottom-left" => (b.x1 + pad, b.y2 - th - pad),
        "bottom-right" => (b.x2 - tw - pad, b.y2 - th - pad),
        _ => (b.x1 + pad, b.y1 + pad), // top-left
    };
    // Keep the label on-screen.
    let tx = tx.clamp(pad, (iw - tw - pad).max(pad));
    let ty = ty.clamp(pad, (ih - th - pad).max(pad));

    if bg_on {
        let rx = (tx - pad).max(0) as i32;
        let ry = (ty - pad).max(0) as i32;
        let rw = (tw + pad * 2).max(1) as u32;
        let rh = (th + pad * 2).max(1) as u32;
        draw_filled_rect_mut(
            &mut rgb,
            Rect::at(rx, ry).of_size(rw, rh),
            image::Rgb(bg_color),
        );
    }
    draw_text_mut(
        &mut rgb,
        image::Rgb(text_color),
        tx as i32,
        ty as i32,
        scale,
        &font,
        &text,
    );

    let out = encode_jpeg(rgb)?;
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(out));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(crate) fn run_blur_outside_bbox(
    config: &Value,
    inputs: &PortValues,
) -> Result<PortValues, String> {
    let raw = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let b = inputs
        .get("bbox")
        .and_then(PortValue::as_bbox)
        .ok_or("missing required input 'bbox'")?;
    let (x1, y1, x2, y2) = (b.x1, b.y1, b.x2, b.y2);
    let sigma = config
        .get("blur")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .or_else(|| v.as_f64().map(|n| n as f32))
        })
        .unwrap_or(12.0)
        .max(0.1);

    // Blur the whole frame, then paste the sharp region of interest back inside
    // the box — everything outside the box ends up blurred.
    let img = image::load_from_memory(raw).map_err(|e| format!("could not decode image: {e}"))?;
    let sharp = img.into_rgb8();
    let mut out_img = image::imageops::blur(&sharp, sigma);
    copy_rect(&sharp, &mut out_img, x1, y1, x2, y2);

    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(out_img)
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Jpeg,
        )
        .map_err(|e| format!("could not encode image: {e}"))?;
    log::debug!(
        "blur_outside_bbox: kept [{x1},{y1},{x2},{y2}] sharp, σ={sigma} → {}-byte JPEG",
        out.len()
    );
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(out));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}
