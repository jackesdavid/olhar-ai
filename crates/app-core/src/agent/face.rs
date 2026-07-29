//! Face embedding for person enrollment & recognition — delegated to the GPU AI
//! sidecar (`ai/sidecar`, op `face_detect_embed`) instead of running SCRFD + ArcFace
//! locally on the CPU (which was ~1 s/frame even with optimised deps). The sidecar
//! detects the most prominent face and returns a 512-d, L2-normalized ArcFace
//! embedding; we store / compare it with pgvector cosine distance.
//!
//! Keeping the same `embed(&[u8]) -> Vec<f32>` signature means the call sites — the
//! `enroll_face` / `recognize_face` Tauri commands and the `RecognizePerson`
//! workflow node — are unchanged; only the compute moved off-device. Configure the
//! sidecar via `AI_SERVICE_URL` / `AI_SERVICE_KEY` (see [`crate::agent::ai_service`]).

use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::ai_service;
use crate::db::Db;

/// ArcFace embedding dimensionality (the pgvector column is `vector(512)`).
pub const EMBED_DIM: usize = 512;

/// One detected face: source-pixel box `[x1,y1,x2,y2]`, detector confidence, and the 5
/// facial landmarks (eyes, nose, mouth corners). The element type of a `FaceDetection[]`
/// (workflow `FaceArray` port). Produced by the sidecar `face_detect` op (a producer node
/// is TBD — the type exists so downstream graphs can be built against it).
#[derive(Debug, Clone)]
pub struct FaceDetection {
    pub bbox: [f32; 4],
    pub confidence: f32,
    pub landmarks: Vec<[f32; 2]>,
}

/// Embed the most prominent face in `image_bytes` via the GPU sidecar.
///
/// Returns a 512-d L2-normalized vector ready for cosine matching. Errors when the
/// sidecar isn't configured, the request fails, or no face is found in the frame.
///
/// Every successful sidecar call is metered against `tenant_id` (the RunPod-billed time)
/// via [`crate::billing`] — including the "no face found" case, which still consumed GPU.
pub fn embed(db: &Db, tenant_id: Uuid, image_bytes: &[u8]) -> Result<Vec<f32>, String> {
    let out = call_detect_embed(db, tenant_id, image_bytes)?;
    if !face_present(&out) {
        return Err("No face found in the image — try a clearer, front-facing photo.".into());
    }
    parse_embedding(&out)
}

/// Embed a face the edge has ALREADY localized (a tight crop from a `face_detected` bbox).
///
/// Tries the full detect+align pipeline first (`face_detect_embed` — best quality, aligns via
/// the detected landmarks). If the sidecar's SCRFD fails to *re-detect* the face inside the crop
/// (common for small, off-angle, or frame-filling faces that the edge's YuNet still found), it
/// falls back to `face_embed` — align-free, resize-to-112 — so a face the edge confidently
/// detected is never silently dropped. Use ONLY when `image_bytes` is a pre-cropped face; for a
/// whole frame (enrollment), use [`embed`], where "no face" is a meaningful error.
pub fn embed_face_crop(db: &Db, tenant_id: Uuid, image_bytes: &[u8]) -> Result<Vec<f32>, String> {
    let out = call_detect_embed(db, tenant_id, image_bytes)?;
    if face_present(&out) {
        return parse_embedding(&out);
    }
    // SCRFD missed it, but the edge already localized this crop — embed it align-free.
    let op = "face_embed";
    let outcome = ai_service::call(
        op,
        json!({ "image_b64": ai_service::image_b64(image_bytes) }),
    )?;
    crate::billing::record_ai_usage(db, tenant_id, op, outcome.delay_ms, outcome.exec_ms, true);
    parse_embedding(&outcome.output)
}

/// Like [`embed_face_crop`], but carries the edge's YuNet landmarks (crop-local pixels) so the
/// sidecar can align the ArcFace crop from THEM when its own SCRFD re-detection misses — the case
/// that today falls back to align-free, resize-to-112 (lower quality). Card #122.
///
/// Non-regressive by construction: the full detect+align pipeline (`face_detect_embed`) is still
/// tried first and wins when SCRFD finds the face (best quality, unchanged). Only on a SCRFD miss do
/// we pass `kps` to `face_embed`; a sidecar that doesn't yet understand `kps` simply ignores the
/// field and embeds align-free exactly as before. Pass fewer than 5 points to force align-free.
pub fn embed_face_crop_aligned(
    db: &Db,
    tenant_id: Uuid,
    image_bytes: &[u8],
    landmarks: &[[f32; 2]],
) -> Result<Vec<f32>, String> {
    let out = call_detect_embed(db, tenant_id, image_bytes)?;
    if face_present(&out) {
        return parse_embedding(&out);
    }
    // SCRFD missed. Embed align-free, but hand the edge's 5 landmarks to the sidecar so it can
    // norm-crop align from them when supported (slice C); older sidecars ignore `kps`.
    let op = "face_embed";
    let mut params = json!({ "image_b64": ai_service::image_b64(image_bytes) });
    if landmarks.len() == 5 {
        params["kps"] = json!(landmarks);
    }
    let outcome = ai_service::call(op, params)?;
    crate::billing::record_ai_usage(db, tenant_id, op, outcome.delay_ms, outcome.exec_ms, true);
    parse_embedding(&outcome.output)
}

/// Detect the most prominent face's 5 landmarks (eyes, nose, mouth corners) in a PRE-CROPPED face
/// image, via the sidecar's `face_detect` op (SCRFD only — cheaper than the embed pipeline).
/// Points are in the image's pixel coords; empty vec if the sidecar re-detected no face (common for
/// tiny/off-angle crops). Billed like other sidecar calls. Used on-demand for the detail overlay.
pub fn detect_landmarks(
    db: &Db,
    tenant_id: Uuid,
    image_bytes: &[u8],
) -> Result<Vec<[f32; 2]>, String> {
    let img_b64 = ai_service::image_b64(image_bytes);
    // Prefer the dense 106-point landmarks (`face_landmarks` → InsightFace `landmark_2d_106`).
    // Fall back to the detector's 5-point `kps` (`face_detect`) on an older sidecar that doesn't
    // know the op, so the overlay still works during a staged deploy.
    if let Ok(outcome) = ai_service::call("face_landmarks", json!({ "image_b64": &img_b64 })) {
        crate::billing::record_ai_usage(
            db,
            tenant_id,
            "face_landmarks",
            outcome.delay_ms,
            outcome.exec_ms,
            true,
        );
        return Ok(parse_points(outcome.output.get("landmarks")));
    }
    let op = "face_detect";
    let outcome = ai_service::call(op, json!({ "image_b64": &img_b64 }))?;
    crate::billing::record_ai_usage(db, tenant_id, op, outcome.delay_ms, outcome.exec_ms, true);
    let first = outcome
        .output
        .get("faces")
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    Ok(parse_points(first.and_then(|f| f.get("kps"))))
}

/// Parse a JSON `[[x,y], …]` landmark array into pixel-coord points; empty on absence/shape mismatch.
fn parse_points(v: Option<&Value>) -> Vec<[f32; 2]> {
    let Some(arr) = v.and_then(Value::as_array) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|p| {
            let a = p.as_array()?;
            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
        })
        .collect()
}

/// Call the sidecar `face_detect_embed` op and meter the (billed) GPU time. Returns the raw
/// output object; the caller decides what a missing face means.
fn call_detect_embed(db: &Db, tenant_id: Uuid, image_bytes: &[u8]) -> Result<Value, String> {
    let op = "face_detect_embed";
    let outcome = ai_service::call(
        op,
        json!({ "image_b64": ai_service::image_b64(image_bytes) }),
    )?;
    // The job ran and was billed by RunPod — record it before we branch on the result.
    crate::billing::record_ai_usage(db, tenant_id, op, outcome.delay_ms, outcome.exec_ms, true);
    Ok(outcome.output)
}

/// Whether the sidecar detected a face in the frame.
fn face_present(out: &Value) -> bool {
    out.get("face").and_then(Value::as_bool).unwrap_or(false)
}

/// Extract + validate the 512-d L2-normalized embedding from a sidecar response.
fn parse_embedding(out: &Value) -> Result<Vec<f32>, String> {
    let embedding: Vec<f32> = out
        .get("embedding")
        .and_then(Value::as_array)
        .ok_or("AI service: face_embedding returned no 'embedding' array")?
        .iter()
        .map(|v| v.as_f64().map(|f| f as f32))
        .collect::<Option<Vec<f32>>>()
        .ok_or("AI service: 'embedding' contained non-numeric values")?;

    if embedding.len() != EMBED_DIM {
        return Err(format!(
            "AI service: expected a {EMBED_DIM}-d embedding, got {}",
            embedding.len()
        ));
    }
    Ok(embedding)
}
