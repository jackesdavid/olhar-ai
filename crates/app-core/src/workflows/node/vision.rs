use super::{PortValue, PortValues, device_from_camera, jpeg_dimensions};
use crate::agent::tools::ToolContext;
use serde_json::Value;

/// Embedded monospace font for the Draw Text node (CascadiaMono).
const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/CascadiaMono-Regular.ttf");

/// Encode an RGB image to JPEG bytes.
fn encode_jpeg(rgb: image::RgbImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| format!("could not encode image: {e}"))?;
    Ok(out)
}

pub(super) fn run_person_detection(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let people = sys.vision.detect_persons_detailed(bytes)?;
    let count = people.len() as i64;
    let confidence = people.iter().map(|p| p.confidence).fold(0.0_f32, f32::max);
    let persons: Vec<PortValue> = people.into_iter().map(PortValue::Person).collect();
    let (w, h) = jpeg_dimensions(bytes);
    let mut o = PortValues::new();
    o.insert("count".into(), PortValue::Num(count));
    o.insert("confidence".into(), PortValue::Num((confidence * 100.0).round() as i64));
    o.insert("persons".into(), PortValue::List(persons));
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(super) fn run_object_detection(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let class_name = inputs
        .get("class")
        .and_then(PortValue::as_text)
        .or_else(|| config.get("class").and_then(|v| v.as_str()))
        .unwrap_or("person");
    let class_id = crate::agent::detector::class_id_for_name(class_name)
        .ok_or_else(|| format!("unknown COCO class: '{class_name}'"))?;
    let detections = sys.vision.detect_objects_detailed(bytes, class_id)?;
    let count = detections.len() as i64;
    let confidence = detections.iter().map(|d| d.confidence).fold(0.0_f32, f32::max);
    let items: Vec<PortValue> = detections.into_iter().map(PortValue::Person).collect();
    let (w, h) = jpeg_dimensions(bytes);
    let mut o = PortValues::new();
    o.insert("count".into(), PortValue::Num(count));
    o.insert("confidence".into(), PortValue::Num((confidence * 100.0).round() as i64));
    o.insert("detections".into(), PortValue::List(items));
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Destructure a PersonDetection element → bbox + confidence.
/// Destructure a `TrackedObject` element (from `GetTrackedObjects`' `objects` array) into its
/// fields — same Unreal-style "Break" shape as `run_break_person_detection`.
pub(super) fn run_break_tracked_object(inputs: &PortValues) -> Result<PortValues, String> {
    let t = inputs
        .get("object")
        .and_then(PortValue::as_tracked_object)
        .ok_or("missing required input 'object'")?;
    let mut o = PortValues::new();
    o.insert("uuid".into(), PortValue::Text(t.uuid.clone()));
    o.insert("class".into(), PortValue::Text(t.class.clone()));
    o.insert("bbox".into(), PortValue::Bbox(t.bbox));
    o.insert("zone".into(), PortValue::Text(t.zone.clone()));
    Ok(o)
}

pub(super) fn run_break_person_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let p = inputs
        .get("person")
        .and_then(PortValue::as_person)
        .ok_or("missing required input 'person'")?;
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::Bbox(bbox_from_xyxy(&p.bbox)));
    o.insert("confidence".into(), PortValue::Num((p.confidence * 100.0).round() as i64));
    Ok(o)
}

/// Destructure a generic Detection element (from Detect Objects) → bbox + confidence.
pub(super) fn run_break_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let d = inputs
        .get("detection")
        .and_then(PortValue::as_person)
        .ok_or("missing required input 'detection'")?;
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::Bbox(bbox_from_xyxy(&d.bbox)));
    o.insert("confidence".into(), PortValue::Num((d.confidence * 100.0).round() as i64));
    Ok(o)
}

/// Destructure a FaceDetection element → bbox + confidence + landmarks.
pub(super) fn run_break_face_detection(inputs: &PortValues) -> Result<PortValues, String> {
    let f = inputs
        .get("face")
        .and_then(PortValue::as_face)
        .ok_or("missing required input 'face'")?;
    let mut o = PortValues::new();
    o.insert("bbox".into(), PortValue::Bbox(bbox_from_xyxy(&f.bbox)));
    o.insert("confidence".into(), PortValue::Num((f.confidence * 100.0).round() as i64));
    o.insert("landmarks".into(), PortValue::Num(f.landmarks.len() as i64));
    Ok(o)
}

pub(super) fn run_recognize_person(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("missing required input 'image'")?;
    let max_distance = config
        .get("max_distance")
        .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(0.4) as f32;
    let embedding = crate::agent::face::embed(&sys.db, sys.tenant_id, bytes)?;
    let matches = sys.recognizer.recognize(&embedding, 5);
    let mut o = PortValues::new();
    match crate::persons::best_match(&matches, max_distance) {
        Some(m) => {
            o.insert("person".into(), PortValue::Text(m.name));
            o.insert("person_id".into(), PortValue::Text(m.person_id));
            o.insert("confidence".into(), PortValue::Num(m.confidence));
            o.insert("ok".into(), PortValue::Bool(true));
        }
        None => {
            o.insert("confidence".into(), PortValue::Num(0));
            o.insert("ok".into(), PortValue::Bool(false));
        }
    }
    Ok(o)
}

// ---- Face recognition pipeline (#117) ----

/// Read a float config value (accepts number or numeric string).
fn cfg_f64(config: &Value, k: &str, d: f64) -> f64 {
    config
        .get(k)
        .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .unwrap_or(d)
}

/// Extract a 512-d face embedding from the image (the model/dim abstraction). `bbox` is accepted
/// for a future crop-to-face; v1 embeds the most prominent face in the frame (the sidecar detects it).
pub(super) fn run_face_embedding(
    _config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("Extract Face Embedding: missing required input 'image'")?;
    let embedding = crate::agent::face::embed(&sys.db, sys.tenant_id, bytes)?;
    let dim = embedding.len() as i64;
    let mut o = PortValues::new();
    o.insert("embedding".into(), PortValue::Embedding(embedding));
    o.insert("dim".into(), PortValue::Num(dim));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Cosine distance → a 0..100 confidence percent.
fn conf_pct(distance: f32) -> i64 {
    (((1.0 - distance.max(0.0)) * 100.0).round().clamp(0.0, 100.0)) as i64
}

/// kNN-recognize an embedding against the tenant's union gallery (enrolled + stored). Outputs the
/// best `person`/`person_id`/`confidence`, a `matches` Dictionary (label → score), and `found`.
pub(super) fn run_recognize_face(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let embedding = inputs
        .get("embedding")
        .and_then(PortValue::as_embedding)
        .ok_or("Recognize Face: missing required input 'embedding'")?;
    let top_k = cfg_i64(config, "top_k", 5).clamp(1, 50);
    let max_distance = cfg_f64(config, "max_distance", 0.5) as f32;
    let matches = crate::faces::recognize(&sys.db, sys.tenant_id, embedding, top_k);

    // matches Dictionary: label (or "unknown:<ref8>") → score. Preserves kNN order (best first).
    let dict: Vec<(String, String)> = matches
        .iter()
        .map(|m| {
            let key = m.label.clone().unwrap_or_else(|| {
                format!("unknown:{}", &m.reference[..m.reference.len().min(8)])
            });
            (key, conf_pct(m.distance).to_string())
        })
        .collect();

    let mut o = PortValues::new();
    o.insert("matches".into(), PortValue::Dict(dict));
    match matches.first() {
        Some(best) => {
            let found = best.distance <= max_distance;
            o.insert("found".into(), PortValue::Bool(found));
            o.insert("confidence".into(), PortValue::Num(conf_pct(best.distance)));
            // person = the name (blank when unknown or below threshold); person_id only when a
            // labeled match won (wire → StoreFace.label).
            let (person, person_id) = if found {
                match &best.label {
                    Some(name) => (name.clone(), best.reference.clone()),
                    None => (String::new(), String::new()),
                }
            } else {
                (String::new(), String::new())
            };
            o.insert("person".into(), PortValue::Text(person));
            o.insert("person_id".into(), PortValue::Text(person_id));
        }
        None => {
            o.insert("found".into(), PortValue::Bool(false));
            o.insert("confidence".into(), PortValue::Num(0));
            o.insert("person".into(), PortValue::Text(String::new()));
            o.insert("person_id".into(), PortValue::Text(String::new()));
        }
    }
    Ok(o)
}

/// Store a face: image → MinIO, embedding → the `faces` table. `label` (a person id) empty = unknown.
/// Skips the insert when an existing stored face is within `dedup_distance` cosine.
pub(super) fn run_store_face(
    config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let embedding = inputs
        .get("embedding")
        .and_then(PortValue::as_embedding)
        .ok_or("Store Face: missing required input 'embedding'")?
        .to_vec();

    let mut o = PortValues::new();
    let dedup = cfg_f64(config, "dedup_distance", 0.15) as f32;
    if dedup > 0.0 {
        if let Some(d) = crate::faces::nearest_stored_distance(&sys.db, sys.tenant_id, &embedding) {
            if d <= dedup {
                o.insert("stored".into(), PortValue::Bool(false));
                return Ok(o);
            }
        }
    }

    let image_key = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .and_then(|bytes| crate::media::put(bytes, "image/jpeg", "jpg").ok());
    let label = inputs
        .get("label")
        .and_then(PortValue::as_text)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let camera = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.id.clone())
        .or_else(|| inputs.get("camera").and_then(PortValue::as_text).map(str::to_string));
    let bbox = inputs.get("bbox").and_then(PortValue::as_bbox).map(|b| {
        serde_json::json!([b.x1, b.y1, b.x2, b.y2])
    });
    let source = config
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("snapshot")
        .to_string();

    let id = crate::faces::insert(
        &sys.db,
        sys.tenant_id,
        crate::faces::NewFace {
            person_id: label,
            embedding,
            image_key,
            camera,
            bbox,
            confidence: 0.0,
            face_ratio: 0.0,
            source,
        },
    )?;
    o.insert("stored".into(), PortValue::Bool(true));
    o.insert("face_id".into(), PortValue::Text(id));
    Ok(o)
}

pub(super) fn run_motion_detection(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let a = inputs.get("image_a").and_then(PortValue::as_image).ok_or("missing required input 'image_a'")?;
    let b = inputs.get("image_b").and_then(PortValue::as_image).ok_or("missing required input 'image_b'")?;
    let pixel_threshold = cfg_i64(config, "pixel_threshold", 25).clamp(0, 255) as i16;
    let sensitivity = cfg_i64(config, "sensitivity", 2).clamp(0, 100) as f64;
    let score = motion_score(a, b, pixel_threshold)?;
    let mut o = PortValues::new();
    o.insert("moved".into(), PortValue::Bool(score >= sensitivity));
    o.insert("score".into(), PortValue::Num(score.round() as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Camera-bound motion detector: resolves its own camera, grabs two frames apart, and reports
/// moved/score. Self-contained per run — drop in a Cron graph to watch a feed continuously.
pub(super) fn run_motion_watch(config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let cam = inputs.get("camera").and_then(PortValue::as_camera)
        .ok_or("missing required camera input")?;
    let device = {
        let devices = sys.devices.lock().unwrap();
        device_from_camera(cam, &devices)?.clone()
    };
    let pixel_threshold = cfg_i64(config, "pixel_threshold", 25).clamp(0, 255) as i16;
    let sensitivity = cfg_i64(config, "sensitivity", 2).clamp(0, 100) as f64;
    let gap_ms = cfg_i64(config, "sample_gap_ms", 400).clamp(0, 5000) as u64;
    let a = sys.frames.snapshot(&device)?;
    std::thread::sleep(std::time::Duration::from_millis(gap_ms));
    let b = sys.frames.snapshot(&device)?;
    let score = motion_score(&a, &b, pixel_threshold)?;
    let mut o = PortValues::new();
    o.insert("moved".into(), PortValue::Bool(score >= sensitivity));
    o.insert("score".into(), PortValue::Num(score.round() as i64));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(super) fn run_get_image_size(inputs: &PortValues) -> Result<PortValues, String> {
    let bytes = inputs.get("image").and_then(PortValue::as_image).ok_or("missing required input 'image'")?;
    let (w, h) = jpeg_dimensions(bytes);
    let mut o = PortValues::new();
    o.insert("width".into(), PortValue::Num(w));
    o.insert("height".into(), PortValue::Num(h));
    o.insert("ok".into(), PortValue::Bool(w > 0 && h > 0));
    Ok(o)
}

pub(super) fn run_draw_bbox(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let raw = inputs.get("image").and_then(PortValue::as_image).ok_or("missing required input 'image'")?;
    let b = inputs.get("bbox").and_then(PortValue::as_bbox).ok_or("missing required input 'bbox'")?;
    let color_str = config.get("color").and_then(|v| v.as_str()).unwrap_or("#00FF00");
    let color = parse_hex_color(color_str).unwrap_or([0, 255, 0]);
    let thickness = config
        .get("thickness")
        .and_then(|v| v.as_str().and_then(|s| s.parse::<u32>().ok()).or_else(|| v.as_u64().map(|n| n as u32)))
        .unwrap_or(2)
        .max(1);
    let style = config.get("style").and_then(|v| v.as_str()).unwrap_or("box");
    // Optional: grow the box outward by `expand`% of its size on each side (draw fns clamp to image).
    let expand = config
        .get("expand")
        .and_then(|v| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()).or_else(|| v.as_i64()))
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
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
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
pub(super) fn run_draw_text(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    use ab_glyph::{FontRef, PxScale};
    use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size};
    use imageproc::rect::Rect;

    let raw = inputs.get("image").and_then(PortValue::as_image).ok_or("missing required input 'image'")?;
    let b = inputs.get("bbox").and_then(PortValue::as_bbox).ok_or("missing required input 'bbox'")?;
    // Text: wired input wins, else the config literal.
    let text = inputs
        .get("text")
        .and_then(PortValue::as_text)
        .map(str::to_string)
        .or_else(|| config.get("text").and_then(|v| v.as_str()).map(str::to_string))
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

    let position = config.get("position").and_then(|v| v.as_str()).unwrap_or("top-left");
    let size = config
        .get("size")
        .and_then(|v| v.as_str().and_then(|s| s.trim().parse::<f32>().ok()).or_else(|| v.as_f64().map(|n| n as f32)))
        .unwrap_or(18.0)
        .clamp(6.0, 200.0);
    let text_color = parse_hex_color(config.get("text_color").and_then(|v| v.as_str()).unwrap_or("#FFFFFF")).unwrap_or([255, 255, 255]);
    let bg_on = config.get("background").and_then(|v| v.as_bool()).unwrap_or(true);
    let bg_color = parse_hex_color(config.get("bg_color").and_then(|v| v.as_str()).unwrap_or("#000000")).unwrap_or([0, 0, 0]);

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
        draw_filled_rect_mut(&mut rgb, Rect::at(rx, ry).of_size(rw, rh), image::Rgb(bg_color));
    }
    draw_text_mut(&mut rgb, image::Rgb(text_color), tx as i32, ty as i32, scale, &font, &text);

    let out = encode_jpeg(rgb)?;
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(out));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

pub(super) fn run_blur_outside_bbox(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let raw = inputs.get("image").and_then(PortValue::as_image).ok_or("missing required input 'image'")?;
    let b = inputs.get("bbox").and_then(PortValue::as_bbox).ok_or("missing required input 'bbox'")?;
    let (x1, y1, x2, y2) = (b.x1, b.y1, b.x2, b.y2);
    let sigma = config
        .get("blur")
        .and_then(|v| v.as_str().and_then(|s| s.trim().parse::<f32>().ok()).or_else(|| v.as_f64().map(|n| n as f32)))
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
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(|e| format!("could not encode image: {e}"))?;
    log::debug!("blur_outside_bbox: kept [{x1},{y1},{x2},{y2}] sharp, σ={sigma} → {}-byte JPEG", out.len());
    let mut o = PortValues::new();
    o.insert("image".into(), PortValue::Image(out));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A detector's `[f32; 4]` xyxy box → the canonical integer `BBox`.
fn bbox_from_xyxy(v: &[f32; 4]) -> crate::workflows::domain::BBox {
    crate::workflows::domain::BBox::from_xyxy(
        v[0].round() as i64,
        v[1].round() as i64,
        v[2].round() as i64,
        v[3].round() as i64,
    )
}

fn motion_score(a: &[u8], b: &[u8], pixel_threshold: i16) -> Result<f64, String> {
    const W: u32 = 320;
    const H: u32 = 240;
    let gray = |raw: &[u8]| -> Result<Vec<u8>, String> {
        let img = image::load_from_memory(raw).map_err(|e| format!("Could not decode frame: {e}"))?;
        Ok(img.resize_exact(W, H, image::imageops::FilterType::Triangle).to_luma8().into_raw())
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

fn cfg_i64(config: &Value, k: &str, d: i64) -> i64 {
    config.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(d)
}

fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

/// Copy the pixels inside the rect from `src` over `dst` (clamped to bounds).
/// Empty when the rect lies fully outside the frame.
fn copy_rect(src: &image::RgbImage, dst: &mut image::RgbImage, x1: i64, y1: i64, x2: i64, y2: i64) {
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

fn draw_rect(img: &mut image::RgbImage, x1: i64, y1: i64, x2: i64, y2: i64, color: [u8; 3], thickness: u32) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let px = image::Rgb(color);
    let t = thickness as i64;
    for d in 0..t {
        for x in x1..=x2 {
            if x >= 0 && x < w {
                if y1 + d >= 0 && y1 + d < h { img.put_pixel(x as u32, (y1 + d) as u32, px); }
                if y2 - d >= 0 && y2 - d < h { img.put_pixel(x as u32, (y2 - d) as u32, px); }
            }
        }
        for y in y1..=y2 {
            if y >= 0 && y < h {
                if x1 + d >= 0 && x1 + d < w { img.put_pixel((x1 + d) as u32, y as u32, px); }
                if x2 - d >= 0 && x2 - d < w { img.put_pixel((x2 - d) as u32, y as u32, px); }
            }
        }
    }
}

/// Fill an inclusive pixel block [ax..=bx, ay..=by], clamped to the frame.
/// No-op when the block is empty or entirely off-frame.
fn fill_block(img: &mut image::RgbImage, ax: i64, ay: i64, bx: i64, by: i64, color: [u8; 3]) {
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
fn draw_corners(img: &mut image::RgbImage, x1: i64, y1: i64, x2: i64, y2: i64, color: [u8; 3], thickness: u32) {
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
fn draw_reticle(img: &mut image::RgbImage, x1: i64, y1: i64, x2: i64, y2: i64, color: [u8; 3], thickness: u32) {
    draw_corners(img, x1, y1, x2, y2, color, thickness);
    let t = thickness.max(1) as i64;
    let cx = (x1 + x2) / 2;
    let cy = (y1 + y2) / 2;
    let half = ((x2 - x1).min(y2 - y1) / 12).clamp(4, 16); // centre square half-size
    // Small centre square outline.
    draw_rect(img, cx - half, cy - half, cx + half, cy + half, color, thickness);
    // Crosshair arms (gap around the square so they don't touch it).
    let gap = half + 4;
    let len = (half * 2).max(8);
    fill_block(img, cx - gap - len, cy, cx - gap, cy + t - 1, color); // left
    fill_block(img, cx + gap, cy, cx + gap + len, cy + t - 1, color); // right
    fill_block(img, cx, cy - gap - len, cx + t - 1, cy - gap, color); // up
    fill_block(img, cx, cy + gap, cx + t - 1, cy + gap + len, color); // down
}
