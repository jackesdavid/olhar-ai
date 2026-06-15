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

use crate::agent::ai_service;

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
pub fn embed(image_bytes: &[u8]) -> Result<Vec<f32>, String> {
    let out = ai_service::call(
        "face_detect_embed",
        json!({ "image_b64": ai_service::image_b64(image_bytes) }),
    )?;

    if !out.get("face").and_then(Value::as_bool).unwrap_or(false) {
        return Err("No face found in the image — try a clearer, front-facing photo.".into());
    }

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
