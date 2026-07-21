//! Edge face detection (#117 Phase 5) — runs YuNet ([`app_core::agent::face_detector`]) on the
//! motion-emit path (already motion-gated + throttled by the hub) and POSTs a `face_detected` event
//! to control → `OnEvent("face_detected")` → the cloud recognition pipeline. Always on: it only runs
//! on qualifying (throttled) motion frames, so the cost is bounded, and a `face_detected` graph just
//! works with no toggle or env to set.
use std::sync::Arc;

use base64::Engine as _;

use crate::state::{AppState, FaceEvent};

/// Detect faces in the just-emitted motion frame and POST **one `face_detected` event per face**
/// (each with its own bbox), so two faces in a frame become two events — the cloud pipeline embeds +
/// recognizes each independently (the embedder crops to the event's bbox). Runs on the hub's
/// already-decoded `bgr` (no re-decode); `jpeg` is the frame relayed on each event. Best-effort.
pub fn detect_and_push(state: &Arc<AppState>, ip: &str, bgr: &[u8], (fw, fh): (u32, u32), jpeg: &[u8]) {
    let faces = match app_core::agent::face_detector::detect_bgr(bgr, fw, fh) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("face detect[{ip}]: {e}");
            return;
        }
    };
    if faces.is_empty() {
        return;
    }
    // Encode the frame once; every face's event carries it (each becomes its own pipeline run).
    let frame_b64 = base64::engine::general_purpose::STANDARD.encode(jpeg);
    let frame_area = (fw as f32 * fh as f32).max(1.0);
    log::info!("face detect[{ip}]: {} face(s)", faces.len());
    for face in &faces {
        let [x1, y1, x2, y2] = face.xyxy;
        // Flatten YuNet's 5 landmarks to `[x0,y0,x1,y1,…]` (source pixels) for the cloud align path.
        let kps: Vec<i32> =
            face.kps.iter().flat_map(|&[x, y]| [x.round() as i32, y.round() as i32]).collect();
        let ev = FaceEvent {
            camera_ip: ip.to_string(),
            bbox: [x1.round() as i32, y1.round() as i32, x2.round() as i32, y2.round() as i32],
            confidence: (face.score * 100.0).clamp(0.0, 100.0) as f64,
            face_ratio: (face.area() / frame_area * 100.0).clamp(0.0, 100.0) as f64,
            kps,
            at: app_core::timeutil::now_rfc3339(),
            frame_b64: Some(frame_b64.clone()),
        };
        state.push_face(&ev);
    }
}
