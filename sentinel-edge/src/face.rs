//! Edge face detection (#117 Phase 5) — runs YuNet ([`app_core::agent::face_detector`]) on the
//! motion-emit path (already motion-gated + throttled by the hub) and POSTs a `face_detected` event
//! to control → `OnEvent("face_detected")` → the cloud recognition pipeline. Always on: it only runs
//! on qualifying (throttled) motion frames, so the cost is bounded, and a `face_detected` graph just
//! works with no toggle or env to set.
use std::sync::Arc;

use base64::Engine as _;

use crate::state::{AppState, FaceEvent};

/// Detect faces in the just-emitted motion frame and, for the most prominent one, POST a
/// `face_detected` event. Best-effort: logs + returns on any error. `(fw, fh)` = frame pixels.
pub fn detect_and_push(state: &Arc<AppState>, ip: &str, jpeg: &[u8], (fw, fh): (u32, u32)) {
    let faces = match app_core::agent::face_detector::detect(jpeg) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("face detect[{ip}]: {e}");
            return;
        }
    };
    // The most prominent (largest) face — v1 emits one event per motion frame.
    let Some(best) = faces.into_iter().max_by(|a, b| a.area().total_cmp(&b.area())) else {
        return;
    };
    let frame_area = (fw as f32 * fh as f32).max(1.0);
    let face_ratio = (best.area() / frame_area * 100.0).clamp(0.0, 100.0);
    let [x1, y1, x2, y2] = best.xyxy;
    let ev = FaceEvent {
        camera_ip: ip.to_string(),
        bbox: [x1.round() as i32, y1.round() as i32, x2.round() as i32, y2.round() as i32],
        confidence: (best.score * 100.0).clamp(0.0, 100.0) as f64,
        face_ratio: face_ratio as f64,
        at: app_core::timeutil::now_rfc3339(),
        frame_b64: Some(base64::engine::general_purpose::STANDARD.encode(jpeg)),
    };
    log::info!("face detect[{ip}]: face conf={:.0}% ratio={:.1}%", best.score * 100.0, face_ratio);
    state.push_face(&ev);
}
