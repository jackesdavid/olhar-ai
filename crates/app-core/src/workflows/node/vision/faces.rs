//! The face pipeline: crop, embed, compare, remember.
//!
//! The order matters and is easy to get wrong. A face is detected on the edge, cropped, turned
//! into an embedding, then compared against the gallery — and only then, if a graph says so, is it
//! stored.
//!
//! Two rules here were learned the hard way and are enforced rather than documented elsewhere:
//!
//! - a match is only `found` when the nearest neighbour is a LABELLED one. An unknown face
//!   matching another unknown face is not a recognition, and firing the True branch on it sends a
//!   graph down the "we know who this is" path when nobody does;
//! - ambiguity is its own answer. When the best and second-best are within a margin of each other,
//!   the honest output is "not sure", not the best guess.
//!
//! Deciding whether a face is good enough to keep belongs UPSTREAM, in the graph — a quality check
//! wired into a branch — never inside the storing node, which only stores.

use super::super::{device_from_camera, jpeg_dimensions, PortValue, PortValues};
use super::*;
use crate::agent::tools::ToolContext;
use serde_json::Value;

pub(crate) fn run_recognize_person(
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
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
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
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(d)
}

/// Extract a 512-d face embedding from the image. When a `bbox` is wired (from
/// `OnEvent("face_detected")`), the frame is **cropped to that face** (+ margin) before embedding —
/// so with several faces in one frame, each per-face event embeds ITS face, not always the largest.
/// Without a bbox, the sidecar embeds the most prominent face in the whole frame.
pub(crate) fn run_face_embedding(
    _config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let bytes = inputs
        .get("image")
        .and_then(PortValue::as_image)
        .ok_or("Extract Face Embedding: missing required input 'image'")?;
    // Crop to the face's bbox when provided (falls back to the whole frame on any decode error).
    // The crop origin lets us translate source-pixel landmarks into the crop's local coordinates.
    let cropped = inputs
        .get("bbox")
        .and_then(PortValue::as_bbox)
        .and_then(|b| crop_to_bbox(bytes, b));
    // The edge's YuNet landmarks (source pixels), when the `face_detected` event carried them (#122).
    let landmarks = inputs
        .get("kps")
        .and_then(PortValue::as_landmarks)
        .unwrap_or(&[]);
    // With a bbox, the edge already localized the face — use the crop-aware embed, which aligns from
    // the edge landmarks (or SCRFD) and falls back to align-free if neither re-detects it (so we
    // never drop a face the edge found). Without a bbox, embed the whole frame ("no face" is real).
    let embedding = match cropped.as_ref() {
        Some((crop, ox, oy)) => {
            let local: Vec<[f32; 2]> = landmarks
                .iter()
                .map(|p| [p[0] - *ox as f32, p[1] - *oy as f32])
                .collect();
            crate::agent::face::embed_face_crop_aligned(&sys.db, sys.tenant_id, crop, &local)?
        }
        None => crate::agent::face::embed(&sys.db, sys.tenant_id, bytes)?,
    };
    let dim = embedding.len() as i64;
    let mut o = PortValues::new();
    o.insert("embedding".into(), PortValue::Embedding(embedding));
    o.insert("dim".into(), PortValue::Num(dim));
    o.insert("ok".into(), PortValue::Bool(true));
    Ok(o)
}

/// Crop a JPEG frame to a face `bbox` expanded by a 30% margin (clamped to the frame), re-encoded as
/// JPEG for the sidecar. Returns `(jpeg, x_off, y_off)` — the crop's top-left in SOURCE pixels, so
/// the caller can translate source-pixel landmarks into the crop's local coordinates. The crop is
/// not resized, so the offset is a pure subtraction. `None` on a decode/encode error → full frame.
fn crop_to_bbox(jpeg: &[u8], b: crate::workflows::domain::BBox) -> Option<(Vec<u8>, i64, i64)> {
    let img = image::load_from_memory(jpeg).ok()?;
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    let (bw, bh) = ((b.x2 - b.x1).max(1), (b.y2 - b.y1).max(1));
    let (mx, my) = (bw * 3 / 10, bh * 3 / 10);
    let x1 = (b.x1 - mx).clamp(0, iw - 1);
    let y1 = (b.y1 - my).clamp(0, ih - 1);
    let x2 = (b.x2 + mx).clamp(x1 + 1, iw);
    let y2 = (b.y2 + my).clamp(y1 + 1, ih);
    let cropped = img.crop_imm(x1 as u32, y1 as u32, (x2 - x1) as u32, (y2 - y1) as u32);
    let mut buf = std::io::Cursor::new(Vec::new());
    cropped.write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
    Some((buf.into_inner(), x1, y1))
}

/// Cosine distance → a 0..100 confidence percent.
fn conf_pct(distance: f32) -> i64 {
    (((1.0 - distance.max(0.0)) * 100.0)
        .round()
        .clamp(0.0, 100.0)) as i64
}

/// kNN-recognize an embedding against the tenant's union gallery (enrolled + stored). Outputs the
/// best `person`/`person_id`/`confidence`, a `matches` Dictionary (label → score), and `found`.
/// Is the winner too close to the runner-up to call? (#123)
///
/// Pure so it can be tested without a database — the accept/reject decision is the part that
/// changes customer-visible behaviour, and it should not need Postgres to prove.
///
/// `margin` is in cosine-distance units. **A margin of 0 must be a no-op**: `best + 0 >= second`
/// is only true when the runner-up is at least as close as the winner, which is genuine ambiguity
/// and was already indistinguishable before this card. That property is what lets #123 ship
/// without changing anyone's results until they opt in.
fn is_ambiguous(best: f32, runner_up: Option<f32>, margin: f32) -> bool {
    runner_up.is_some_and(|second| best + margin >= second)
}

pub(crate) fn run_recognize_face(
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
    // #123. How much closer the winner must be than the next DIFFERENT identity. 0 keeps the old
    // behaviour exactly, so this cannot change anyone's results until they opt in — the default
    // must stay 0 until [[face-threshold-calibration]] measures a real one, because a margin
    // guessed here would silently start rejecting true matches.
    let margin = cfg_f64(config, "margin", 0.0) as f32;

    // Score per IDENTITY, not per embedding: a person with many enrolled crops could otherwise win
    // on a single outlier, and one bad enrollment crop could make a true person lose.
    let people = crate::faces::recognize_people(&sys.db, sys.tenant_id, embedding, top_k);

    // matches Dictionary: label (or "unknown:<ref8>") → score. Preserves order (best identity first).
    let dict: Vec<(String, String)> = people
        .iter()
        .map(|m| {
            let key = m
                .label
                .clone()
                .unwrap_or_else(|| format!("unknown:{}", &m.reference[..m.reference.len().min(8)]));
            (key, conf_pct(m.best).to_string())
        })
        .collect();

    let mut o = PortValues::new();
    o.insert("matches".into(), PortValue::Dict(dict));

    // `found` means "recognized a KNOWN (labeled) person". The union gallery also holds stored
    // UNKNOWN faces (label = None) for dedup/re-id — a nearest match to one of those must NOT count
    // as recognized (else every unknown that resembles a prior unknown fires the True branch). So
    // pick the nearest LABELED identity within threshold, not just the nearest anything.
    let best_labeled = people
        .iter()
        .find(|m| m.label.is_some() && m.best <= max_distance);

    // The runner-up is the nearest identity that is NOT the winner — labeled or not. An unknown
    // sitting just as close as the winner is exactly the ambiguity this test exists to catch, so it
    // counts too.
    let runner_up = |winner: &crate::faces::PersonScore| {
        people
            .iter()
            .find(|m| m.reference != winner.reference)
            .map(|m| m.best)
    };

    match best_labeled {
        Some(m) => {
            let second = runner_up(m);
            // Ambiguous when the runner-up is within `margin` of the winner. With margin = 0 this
            // is never true, which is why the default is a no-op.
            let ambiguous = is_ambiguous(m.best, second, margin);
            o.insert("found".into(), PortValue::Bool(!ambiguous));
            o.insert("confidence".into(), PortValue::Num(conf_pct(m.best)));
            o.insert("consistency".into(), PortValue::Num(conf_pct(m.mean_top3)));
            o.insert("samples".into(), PortValue::Num(m.n as i64));
            o.insert(
                "margin".into(),
                PortValue::Num(
                    second
                        .map_or(100, |d| conf_pct(m.best) - conf_pct(d))
                        .max(0),
                ),
            );
            o.insert("ambiguous".into(), PortValue::Bool(ambiguous));
            // On an ambiguous result the identity is still reported — a graph may want to show
            // "probably X" — but `found` is false, so the True branch does not fire.
            o.insert(
                "person".into(),
                PortValue::Text(m.label.clone().unwrap_or_default()),
            );
            o.insert("person_id".into(), PortValue::Text(m.reference.clone()));
        }
        None => {
            // No labeled identity within threshold → unknown. Report the nearest distance (any
            // kind) as confidence context; found=false → the False branch stores it as unknown.
            let nearest = people.first();
            o.insert("found".into(), PortValue::Bool(false));
            o.insert(
                "confidence".into(),
                PortValue::Num(nearest.map_or(0, |m| conf_pct(m.best))),
            );
            o.insert(
                "consistency".into(),
                PortValue::Num(nearest.map_or(0, |m| conf_pct(m.mean_top3))),
            );
            o.insert(
                "samples".into(),
                PortValue::Num(nearest.map_or(0, |m| m.n as i64)),
            );
            o.insert("margin".into(), PortValue::Num(0));
            o.insert("ambiguous".into(), PortValue::Bool(false));
            o.insert("person".into(), PortValue::Text(String::new()));
            o.insert("person_id".into(), PortValue::Text(String::new()));
        }
    }
    Ok(o)
}

/// Store a face: image → MinIO, embedding → the `faces` table. `label` (a person id) empty = unknown.
/// Skips the insert when an existing stored face is within `dedup_distance` cosine.
pub(crate) fn run_store_face(
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

    // Detection score + face-size (from the face_detected event). StoreFace only STORES them — it
    // makes no quality decision. Deciding WHICH faces to store belongs upstream in the graph
    // (e.g. When Event → NumGte on face_width → Branch → StoreFace).
    let confidence = inputs
        .get("confidence")
        .and_then(PortValue::as_num)
        .unwrap_or(0) as f32;
    let face_width = inputs
        .get("face_width")
        .and_then(PortValue::as_num)
        .unwrap_or(0) as i32;

    // Quality signals (#126) — recorded, not judged. An unwired port stays NULL rather than 0:
    // "not measured" and "measured as zero" are different facts, and only the first should be
    // invisible to threshold calibration (#128). The fail-open sentinel control substitutes for an
    // edge too old to measure sharpness is mapped back to NULL here for the same reason — it means
    // "unknown", and must not enter the data as an excellent score.
    let quality = |name: &str| {
        inputs
            .get(name)
            .and_then(PortValue::as_num)
            .map(|v| v as f32)
    };
    let blur = quality("blur").filter(|b| (*b as f64) < crate::agent::face_quality::BLUR_UNKNOWN);
    let (yaw, pitch, roll) = (quality("yaw"), quality("pitch"), quality("roll"));

    let dedup = cfg_f64(config, "dedup_distance", 0.15) as f32;
    if dedup > 0.0 {
        if let Some(d) = crate::faces::nearest_stored_distance(&sys.db, sys.tenant_id, &embedding) {
            if d <= dedup {
                o.insert("stored".into(), PortValue::Bool(false));
                return Ok(o);
            }
        }
    }

    // The stored image is the whole FRAME (kept for the Context / full-frame+box view).
    let frame = inputs.get("image").and_then(PortValue::as_image);
    let image_key = frame.and_then(|b| crate::media::put_face(b, "image/jpeg", "jpg").ok());
    let bb = inputs.get("bbox").and_then(PortValue::as_bbox);
    let bbox = bb.map(|b| serde_json::json!([b.x1, b.y1, b.x2, b.y2]));
    // Pre-crop the face → a small passthrough thumbnail so the image proxy serves reads WITHOUT
    // decoding + cropping the frame each time (the read-concurrency fix). None → crop on read.
    let crop_bytes = match (frame, bb) {
        (Some(f), Some(b)) => crate::faces::render_face_image(f, [b.x1, b.y1, b.x2, b.y2], false),
        _ => None,
    };
    let crop_key = crop_bytes
        .as_deref()
        .and_then(|c| crate::media::put_face(c, "image/jpeg", "jpg").ok());

    // Landmarks, stored ONCE here instead of recomputed on every view (migration 0092).
    //
    // The detail overlay used to call the sidecar on each open — one inference per view, for points
    // that never change, and an overlay that silently emptied whenever the sidecar was down. A face
    // is stored once and looked at many times, so paying here is the right side of that ratio.
    //
    // Dense (InsightFace 106) when the sidecar answers, else the edge's own 5 YuNet keypoints if
    // the graph wired them. The source is recorded because the two sets are NOT interchangeable: a
    // viewer expecting 106 that receives 5 draws five dots and looks broken.
    let wired_kps = inputs.get("kps").and_then(PortValue::as_landmarks);
    let (landmarks, landmarks_source) = match crop_bytes
        .as_deref()
        .and_then(|c| crate::agent::face::detect_landmarks(&sys.db, sys.tenant_id, c).ok())
        .filter(|p| !p.is_empty())
    {
        Some(pts) => (
            Some(serde_json::json!(pts)),
            Some("insightface_106".to_string()),
        ),
        None => match wired_kps {
            Some(k) if !k.is_empty() => (Some(serde_json::json!(k)), Some("yunet_5".to_string())),
            _ => (None, None),
        },
    };
    let label = inputs
        .get("label")
        .and_then(PortValue::as_text)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let camera = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.id.clone())
        .or_else(|| {
            inputs
                .get("camera")
                .and_then(PortValue::as_text)
                .map(str::to_string)
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
            confidence,
            face_width,
            source,
            crop_key,
            blur,
            yaw,
            pitch,
            roll,
            landmarks,
            landmarks_source,
        },
    )?;
    o.insert("stored".into(), PortValue::Bool(true));
    o.insert("face_id".into(), PortValue::Text(id));
    Ok(o)
}

#[cfg(test)]
mod recognize_scoring_tests {
    use super::is_ambiguous;

    /// The compatibility guarantee: margin 0 changes nothing for a clear winner.
    #[test]
    fn margin_zero_is_a_no_op_for_a_clear_winner() {
        assert!(!is_ambiguous(0.20, Some(0.45), 0.0));
        assert!(!is_ambiguous(0.20, None, 0.0));
    }

    /// A runner-up at least as close as the winner is ambiguous even with no margin — that case
    /// was always a coin flip, it just used to be silently resolved in the winner's favour.
    #[test]
    fn a_tie_is_ambiguous_even_at_margin_zero() {
        assert!(is_ambiguous(0.30, Some(0.30), 0.0));
        assert!(is_ambiguous(0.30, Some(0.29), 0.0));
    }

    /// With a margin, a winner must be clear of the runner-up by at least that much.
    #[test]
    fn margin_rejects_a_winner_that_is_barely_ahead() {
        // 0.05 clear, margin demands 0.10 → not good enough.
        assert!(is_ambiguous(0.30, Some(0.35), 0.10));
        // 0.15 clear, margin demands 0.10 → accepted.
        assert!(!is_ambiguous(0.30, Some(0.45), 0.10));
    }

    /// Exactly at the margin is ambiguous, not accepted. `>=` on purpose: the boundary case is the
    /// one most likely to be a false accept, so it should fail closed.
    #[test]
    fn the_boundary_fails_closed() {
        assert!(is_ambiguous(0.30, Some(0.40), 0.10));
    }

    /// No runner-up at all — a single identity in the gallery — can never be ambiguous.
    #[test]
    fn a_lone_candidate_is_never_ambiguous() {
        for margin in [0.0, 0.1, 0.5, 10.0] {
            assert!(!is_ambiguous(0.30, None, margin));
        }
    }
}
