use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use serde_json::Value;

/// Edge recording calls go over the tailnet and must not hang the node (a bare `ureq::post`
/// has no timeout). Cap below the node watchdog (15s) so a stalled edge fails fast with a
/// clean error instead of leaking the watchdog's detached worker thread.
fn edge_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(12))
        .build()
}

pub(super) fn run_start_recording(_config: &Value, inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let camera_ip = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.ip.clone())
        .ok_or("missing required input 'camera'")?;
    let pre_roll = inputs
        .get("pre_roll_secs")
        .and_then(PortValue::as_num)
        .map(|n| n as u64);
    let base = resolve_edge_api_base(sys)?;
    let mut body = serde_json::json!({ "camera_ip": camera_ip });
    if let Some(pr) = pre_roll {
        body["pre_roll_secs"] = serde_json::Value::Number(pr.into());
    }
    let resp = edge_agent()
        .post(&format!("{base}/recording/start"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("edge start_recording: {e}"))?;
    let json = resp.into_json::<serde_json::Value>().ok().unwrap_or_default();
    let clip_id: String = json
        .get("clip_id").and_then(|s| s.as_str()).map(str::to_string)
        .unwrap_or_else(|| format!("{camera_ip}-{}", chrono::Utc::now().timestamp()));
    let is_new = json.get("new").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut o = PortValues::new();
    o.insert("clip_id".into(), PortValue::Text(clip_id));
    o.insert("recording_new".into(), PortValue::Bool(is_new));
    Ok(o)
}

pub(super) fn run_stop_recording(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let camera_ip = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.ip.clone())
        .ok_or("missing required input 'camera'")?;
    let clip_id = inputs
        .get("clip_id")
        .and_then(PortValue::as_text)
        .unwrap_or("")
        .to_string();
    let base = resolve_edge_api_base(sys)?;
    let body = serde_json::json!({ "camera_ip": camera_ip, "clip_id": clip_id });
    edge_agent()
        .post(&format!("{base}/recording/stop"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("edge stop_recording: {e}"))?;
    Ok(PortValues::new())
}

fn resolve_edge_base(sys: &ToolContext) -> Result<String, String> {
    let pool = sys.db.pool();
    let tid = sys.tenant_id;
    let host: Option<String> = sys.db.block(async move {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT magicdns_host FROM tenant_boxes
              WHERE tenant_id = $1 AND magicdns_host IS NOT NULL
           ORDER BY last_seen DESC NULLS LAST LIMIT 1",
        )
        .bind(tid)
        .fetch_optional(&pool)
        .await
        .unwrap_or(None);
        row.and_then(|(h,)| h)
    });
    let h = host.ok_or("no edge box connected for this tenant")?;
    let trimmed = h.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("https://{trimmed}"))
    }
}

/// Returns the edge API base URL (port 8080 via tailscale serve).
/// go2rtc occupies port 443; the edge API is served separately on 8080.
fn resolve_edge_api_base(sys: &ToolContext) -> Result<String, String> {
    let base = resolve_edge_base(sys)?;
    let bare = base
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    Ok(format!("https://{bare}:8080"))
}
