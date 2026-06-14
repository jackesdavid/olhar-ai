//! Stream — Tauri adapter for the live-feed slice. The app has no go2rtc of its own.
//! All camera media flows through the **sentinel-gateway** (which reaches the on-prem
//! **sentinel-edge** go2rtc over the Tailscale tailnet) — the same path locally and
//! in production. This slice is a thin client of the gateway: `capture_snapshot` and
//! `ensure_streams` proxy to it. Configure with `GATEWAY_URL`
//! (default `http://127.0.0.1:8789`).
use std::io::Read;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

use app_core::go2rtc::stream_name;

/// The sentinel-gateway base URL — the single entry point for live video + snapshots.
fn gateway_base() -> String {
    std::env::var("GATEWAY_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8789".to_string())
}

// ---- commands ---------------------------------------------------------------

/// A JPEG snapshot for the camera at `ip`, fetched through the gateway
/// (`/api/frame.jpeg?src=cam_<ip>`) and returned as a `data:image/jpeg;base64,…`
/// URL ready for an `<img src>`. Blocking HTTP on a task.
#[tauri::command]
pub async fn capture_snapshot(ip: String) -> Result<String, String> {
    let url = format!("{}/api/frame.jpeg?src={}", gateway_base(), stream_name(&ip));
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let resp = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .build()
            .get(&url)
            .call()
            .map_err(|e| format!("gateway snapshot failed: {e}"))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("reading gateway snapshot: {e}"))?;
        Ok::<Vec<u8>, String>(buf)
    })
    .await
    .map_err(|e| format!("Snapshot task failed: {e}"))??;

    log::info!("capture_snapshot: ip={ip} — {} bytes via gateway", bytes.len());
    Ok(format!("data:image/jpeg;base64,{}", B64.encode(&bytes)))
}

/// One playable stream for the Live screen. `player_url` points at the gateway's MSE
/// player so live video flows over the same gateway→tailnet path as everything else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    ip: String,
    /// go2rtc stream name, e.g. `cam_192_168_1_50`.
    name: String,
    /// The gateway MSE player URL for this stream (`/player?src=<name>`).
    player_url: String,
}

/// The playable streams, discovered via the gateway (`GET /api/streams`, proxied to
/// the edge's go2rtc). One entry per configured stream, with a gateway player URL.
#[tauri::command]
pub async fn ensure_streams() -> Result<Vec<StreamInfo>, String> {
    let gw = gateway_base();
    let url = format!("{gw}/api/streams");
    let names = tauri::async_runtime::spawn_blocking(move || {
        let v = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()
            .get(&url)
            .call()
            .map_err(|e| format!("gateway streams failed: {e}"))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("parsing gateway streams: {e}"))?;
        // go2rtc's /api/streams is an object keyed by stream name.
        let names: Vec<String> = v.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        Ok::<Vec<String>, String>(names)
    })
    .await
    .map_err(|e| format!("Streams task failed: {e}"))??;

    let streams: Vec<StreamInfo> = names
        .into_iter()
        .map(|name| {
            // Reverse `cam_192_168_0_81` → `192.168.0.81` for the UI's device link.
            let ip = name.strip_prefix("cam_").map(|s| s.replace('_', ".")).unwrap_or_else(|| name.clone());
            let player_url = format!("{gw}/player?src={name}");
            StreamInfo { ip, name, player_url }
        })
        .collect();

    log::info!("ensure_streams: {} stream(s) via gateway", streams.len());
    Ok(streams)
}
