//! Stream — Tauri adapter for the live-feed slice. The app no longer runs go2rtc:
//! the on-prem **sentinel-edge** service owns it and is the only thing that touches
//! cameras. This slice is a thin client of the edge — `capture_snapshot` and
//! `ensure_streams` proxy to it. Configure the edge with `EDGE_URL` (default
//! `http://127.0.0.1:8788`).
use std::io::Read;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

/// The sentinel-edge REST base URL. Streams + snapshots come from there.
fn edge_base() -> String {
    std::env::var("EDGE_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8788".to_string())
}

// ---- commands ---------------------------------------------------------------

/// A JPEG snapshot for the camera at `ip`, fetched from the edge and returned as a
/// `data:image/jpeg;base64,…` URL ready for an `<img src>`. Blocking HTTP on a task.
#[tauri::command]
pub async fn capture_snapshot(ip: String) -> Result<String, String> {
    let url = format!("{}/snapshot/{ip}", edge_base());
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let resp = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .build()
            .get(&url)
            .call()
            .map_err(|e| format!("edge snapshot failed: {e}"))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("reading edge snapshot: {e}"))?;
        Ok::<Vec<u8>, String>(buf)
    })
    .await
    .map_err(|e| format!("Snapshot task failed: {e}"))??;

    log::info!("capture_snapshot: ip={ip} — {} bytes from edge", bytes.len());
    Ok(format!("data:image/jpeg;base64,{}", B64.encode(&bytes)))
}

/// One playable stream for the Live screen (as returned by the edge's `/streams`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    ip: String,
    /// go2rtc stream name, e.g. `cam_192_168_1_50`.
    name: String,
    /// go2rtc's ready-made auto-negotiating player URL for this stream.
    player_url: String,
}

/// The playable streams, from the edge (which owns go2rtc). Blocking HTTP on a task.
#[tauri::command]
pub async fn ensure_streams() -> Result<Vec<StreamInfo>, String> {
    let url = format!("{}/streams", edge_base());
    let streams = tauri::async_runtime::spawn_blocking(move || {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()
            .get(&url)
            .call()
            .map_err(|e| format!("edge streams failed: {e}"))?
            .into_json::<Vec<StreamInfo>>()
            .map_err(|e| format!("parsing edge streams: {e}"))
    })
    .await
    .map_err(|e| format!("Streams task failed: {e}"))??;

    log::info!("ensure_streams: {} stream(s) from edge", streams.len());
    Ok(streams)
}
