use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use serde_json::Value;

/// Edge recording calls go over the tailnet and must not hang the node (a bare `ureq::post`
/// has no timeout). Cap below the node watchdog (15s) so a stalled edge fails fast with a
/// clean error instead of leaking the watchdog's detached worker thread.
pub(super) fn edge_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(12))
        .build()
}

pub(super) fn run_start_recording(
    _config: &Value,
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
    let camera_ip = inputs
        .get("camera")
        .and_then(PortValue::as_camera)
        .map(|c| c.ip.clone())
        .ok_or("missing required input 'camera'")?;
    let pre_roll = inputs
        .get("pre_roll_secs")
        .and_then(PortValue::as_num)
        .map(|n| n as u64);
    let mut body = serde_json::json!({ "camera_ip": camera_ip });
    if let Some(pr) = pre_roll {
        body["pre_roll_secs"] = serde_json::Value::Number(pr.into());
    }
    let json = edge_call(sys, &camera_ip, "start_recording", "/recording/start", body)?;
    let clip_id: String = json
        .get("clip_id")
        .and_then(|s| s.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{camera_ip}-{}", chrono::Utc::now().timestamp()));
    let is_new = json.get("new").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut o = PortValues::new();
    o.insert("clip_id".into(), PortValue::Text(clip_id));
    o.insert("recording_new".into(), PortValue::Bool(is_new));
    Ok(o)
}

pub(super) fn run_stop_recording(
    inputs: &PortValues,
    sys: &ToolContext,
) -> Result<PortValues, String> {
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
    let body = serde_json::json!({ "camera_ip": camera_ip, "clip_id": clip_id });
    edge_call(sys, &camera_ip, "stop_recording", "/recording/stop", body)?;
    Ok(PortValues::new())
}

/// The tenant's box that is reachable over the SSE command stream right now (fresh `sse_last_seen`),
/// if any. Its presence is what routes a command over SSE instead of the legacy inbound POST.
fn sse_box(sys: &ToolContext, camera_ip: &str) -> Option<uuid::Uuid> {
    let pool = sys.db.pool();
    let tid = sys.tenant_id;
    let ip = camera_ip.to_string();
    sys.db
        .block(async move {
            // The box that OWNS this camera, if one has claimed it (#175, migration 0091).
            //
            // This used to pick whichever box pinged most recently, for ANY camera — so a recording
            // could be handed to a box that has never seen the camera in question. That produced a
            // real, shipped artefact: recording the phone's own camera was routed to the Mac, which
            // has the camera in its list but none of its frames, so the ring stayed empty and the
            // clip finalised at 587 bytes in the user's recordings list.
            //
            // Ownership is the answer to "who can actually record this", and it is the same answer
            // as "who may dial it" — one camera, one consumer.
            let owner = sqlx::query_scalar::<_, Option<uuid::Uuid>>(
                "SELECT box_id FROM devices WHERE tenant_id = $1 AND ip = $2",
            )
            .bind(tid)
            .bind(&ip)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
            .flatten();
            if let Some(owner) = owner {
                return Ok(Some(owner));
            }
            // Unclaimed: fall back to the most recently seen box, which is the pre-#175 behaviour.
            // A camera nobody has claimed is one no box has proven it is reading, so there is no
            // better answer available — but it is a guess, and it is now only made when it must be.
            sqlx::query_scalar::<_, uuid::Uuid>(
                "SELECT id FROM tenant_boxes
                 WHERE tenant_id = $1 AND sse_last_seen IS NOT NULL
                   AND sse_last_seen::timestamptz > now() - make_interval(secs => 30)
                 ORDER BY sse_last_seen DESC LIMIT 1",
            )
            .bind(tid)
            .fetch_optional(&pool)
            .await
        })
        .ok()
        .flatten()
}

/// Deliver an edge recording command by the right transport, and return the edge's JSON reply.
///
/// **Card #144 — versioned + backward compatible.** If the tenant has a live SSE command stream, the
/// command rides *down* it (works for an outbound-only box the cloud can't reach inbound — a phone,
/// and eventually every edge once the tailnet is retired). Otherwise it takes the historical
/// **tailnet POST** to `magicdns_host:8080`, unchanged — so existing appliance boxes behave exactly
/// as before. `cmd` is the SSE command name; `path` is the equivalent local HTTP endpoint.
fn edge_call(
    sys: &ToolContext,
    camera_ip: &str,
    cmd: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if let Some(box_id) = sse_box(sys, camera_ip) {
        // 20s TTL so a command outlives the await window a touch (redelivery/late run); the await
        // itself stays under the node's 15s watchdog.
        let id =
            crate::edge_commands::enqueue_blocking(&sys.db, sys.tenant_id, box_id, cmd, body, 20)
                .ok_or("edge command enqueue failed")?;
        crate::edge_commands::await_result_blocking(&sys.db, id, 12)
    } else {
        let base = resolve_edge_api_base(sys)?;
        let resp = edge_agent()
            .post(&format!("{base}{path}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("edge {cmd}: {e}"))?;
        Ok(resp
            .into_json::<serde_json::Value>()
            .ok()
            .unwrap_or_default())
    }
}

/// Cap on a fetched clip — Telegram rejects >50 MB for bot uploads anyway, and it bounds memory.
const MAX_CLIP_BYTES: u64 = 50 * 1024 * 1024;

/// Download a finished recording clip's mp4 bytes from this tenant's edge (card #15, used by
/// `Reply with recording`). `GET {edge_api}/recording/{clip_id}`. The read is size-bounded so a
/// runaway response can't exhaust memory.
pub(super) fn fetch_clip(sys: &ToolContext, clip_id: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let base = resolve_edge_api_base(sys)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(90))
        .build();
    let resp = agent
        .get(&format!("{base}/recording/{clip_id}"))
        .call()
        .map_err(|e| format!("edge fetch clip {clip_id}: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MAX_CLIP_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read clip {clip_id}: {e}"))?;
    if buf.is_empty() {
        return Err(format!(
            "clip {clip_id} was empty (not found or still recording?)"
        ));
    }
    if buf.len() as u64 > MAX_CLIP_BYTES {
        return Err(format!(
            "clip {clip_id} exceeds {MAX_CLIP_BYTES} bytes — too large to relay"
        ));
    }
    Ok(buf)
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
pub(super) fn resolve_edge_api_base(sys: &ToolContext) -> Result<String, String> {
    let base = resolve_edge_base(sys)?;
    let bare = base
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    Ok(format!("https://{bare}:8080"))
}
