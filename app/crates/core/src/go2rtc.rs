//! The shared live feed: go2rtc holds one persistent RTSP connection per camera
//! and multiplexes it to every consumer (the Live page, snapshots, every running
//! graph). This module is the single source of truth for the sidecar's base URL
//! and stream naming, plus a fast "grab the current frame" helper used by snapshots
//! instead of a from-scratch ONVIF/ffmpeg capture.
use std::io::Read;
use std::time::Duration;

/// The go2rtc sidecar's local HTTP API (also the WebRTC/MSE player + frame API).
pub const BASE: &str = "http://127.0.0.1:1984";

/// The go2rtc stream name for a camera `ip`. MUST match the names written into the
/// go2rtc config (`build_go2rtc_yaml`) and handed to the Live player (`ensure_streams`)
/// — otherwise the snapshot `src` won't resolve.
pub fn stream_name(ip: &str) -> String {
    format!("cam_{}", ip.replace('.', "_"))
}

/// The current JPEG frame from the shared go2rtc feed for camera `ip`.
///
/// Fast (~tens of ms) when the stream is already warm. If the sidecar isn't running
/// the localhost request fails immediately (connection refused), so callers can fall
/// back to a from-scratch capture without paying a timeout.
pub fn frame(ip: &str) -> Result<Vec<u8>, String> {
    let url = format!("{BASE}/api/frame.jpeg?src={}", stream_name(ip));
    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .build()
        .get(&url)
        .call()
        .map_err(|e| format!("go2rtc frame request failed: {e}"))?;

    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading go2rtc frame: {e}"))?;
    if bytes.is_empty() {
        return Err("go2rtc returned an empty frame".into());
    }
    Ok(bytes)
}
