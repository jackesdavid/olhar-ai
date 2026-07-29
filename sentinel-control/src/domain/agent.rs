//! Conversational assistant on the cloud: a chat turn (Claude + the agent tool loop),
//! text-to-speech, and speech-to-text. All three are JWT-authenticated and tenant-scoped —
//! the tool loop only ever touches the caller's tenant (the [`ToolContext`] is built from
//! `claims.tid`, never the client). The agent reaches the tenant's cameras through an
//! [`EdgeFrameSource`] bound to that tenant's edge go2rtc over the tailnet, exactly like a
//! workflow run does (see `sentinel-flow` `build_run_context`).
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::post;
use axum::Router;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use app_core::agent::domain::Message;
use app_core::agent::tools::{ToolContext, ToolRegistry, ToolUiEvent};
use app_core::notification::{NotificationConfig, NotificationStore};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agent/chat", post(chat))
        .route("/agent/tts", post(tts))
        .route("/agent/stt", post(stt))
}

// ---- chat -------------------------------------------------------------------

#[derive(Deserialize)]
struct ChatTurn {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatReq {
    history: Vec<ChatTurn>,
}

#[derive(Serialize)]
struct ChatResp {
    text: String,
    tool_events: Vec<Value>,
}

/// Run one conversation turn: rebuild the history, run the agent's tool loop against the
/// tenant's live state, and return the reply plus any inline tool effects (snapshots, etc.).
/// Stateless — the browser keeps the transcript and re-sends it each turn.
async fn chat(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(req): Json<ChatReq>,
) -> Result<Json<ChatResp>, ApiErr> {
    auth::require(&c, "assistant.use")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();

    let resp = blocking(move || {
        // Stores freshly loaded from Postgres (devices/alerts/beacons can change between turns).
        let devices = Arc::new(std::sync::Mutex::new(app_core::devices::repo::load(
            &db, tid,
        )));
        let notif: NotificationStore = Arc::new(std::sync::Mutex::new(
            app_core::notification::repo::load_beacons(&db, tid),
        ));
        let notif_cfg = resolve_telegram_config(&db, tid);

        // Collect the tool layer's UI side-effects (snapshots, "list changed" pokes…) so we can
        // render them inline in the reply. The agent runs synchronously, so by the time `ask`
        // returns every event is already queued on this channel.
        let (tx, rx) = std::sync::mpsc::channel::<ToolUiEvent>();
        let mut ctx = ToolContext::new_local(devices, db.clone(), tid, notif, notif_cfg, tx);
        // Bind frames to THIS tenant's edge go2rtc (over the tailnet), so camera tools work in
        // the cloud — falls back to the configured/localhost go2rtc when no box is known yet.
        let base = match app_core::tenancy::box_host_for_tenant(&db, tid) {
            Some(host) => https_base(&host),
            None => app_core::go2rtc::base(),
        };
        ctx.frames = Arc::new(app_core::ports::EdgeFrameSource::new(base));

        let history: Vec<Message> = req
            .history
            .iter()
            .map(|t| match t.role.as_str() {
                "assistant" => Message::assistant(t.content.clone()),
                _ => Message::user(t.content.clone()),
            })
            .collect();

        let registry = ToolRegistry::new();
        let text = match app_core::agent::client::ask(&history, &registry, &ctx, false) {
            Some(outcome) => outcome.reply_text,
            None => {
                "The assistant isn't reachable right now — check that ANTHROPIC_API_KEY is set."
                    .to_string()
            }
        };
        drop(ctx); // release the Sender so the drain below terminates cleanly

        let tool_events: Vec<Value> = rx.into_iter().filter_map(event_json).collect();
        ChatResp { text, tool_events }
    })
    .await?;

    Ok(Json(resp))
}

/// Resolve the tenant's Telegram config (saved config if it has a token, else none). Mirrors
/// `sentinel-flow`'s `resolve_telegram_config` so `send_notification` works the same in chat.
fn resolve_telegram_config(
    db: &app_core::db::Db,
    tenant: uuid::Uuid,
) -> Option<NotificationConfig> {
    // tenant-own bot → shared GLOBAL provider bot → env (was tenant→none, skipping the global bot).
    app_core::notification::repo::resolve_effective_telegram(db, tenant)
}

/// Normalize a stored box host into a usable base URL with exactly one scheme.
fn https_base(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") {
        h.to_string()
    } else {
        format!("https://{h}")
    }
}

/// Map a tool UI side-effect to the JSON the chat page renders inline. Image-bearing events
/// are inlined as base64 data URLs (no second round-trip). Events with no chat representation
/// (e.g. map `Presence`) are dropped.
fn event_json(ev: ToolUiEvent) -> Option<Value> {
    let data_url = |jpeg: &[u8]| {
        format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(jpeg)
        )
    };
    Some(match ev {
        ToolUiEvent::Snapshot { camera, jpeg } => {
            json!({ "type": "snapshot", "camera": camera, "image": data_url(&jpeg) })
        }
        ToolUiEvent::ShowImage { label, jpeg, .. } => {
            json!({ "type": "image", "label": label, "image": data_url(&jpeg) })
        }
        ToolUiEvent::OpenLive { ip } => json!({ "type": "open_live", "ip": ip }),
        ToolUiEvent::DevicesChanged => json!({ "type": "devices_changed" }),
        ToolUiEvent::AlertsChanged => json!({ "type": "alerts_changed" }),
        ToolUiEvent::Print { message, level, .. } => {
            json!({ "type": "print", "message": message, "level": level })
        }
        ToolUiEvent::Presence { .. } => return None,
        // Workflow display-node value — editor-only (SSE), not surfaced in the chat agent.
        ToolUiEvent::ShowValue { .. } => return None,
    })
}

// ---- text-to-speech ---------------------------------------------------------

#[derive(Deserialize)]
struct TtsReq {
    text: String,
}

/// Synthesize the assistant's reply to MP3 (ElevenLabs). 503 when no voice key is configured.
async fn tts(UserClaims(c): UserClaims, Json(req): Json<TtsReq>) -> Result<Response, ApiErr> {
    auth::require(&c, "assistant.use")?;
    let mp3 = blocking(move || app_core::voice::synthesize(&req.text)).await?;
    match mp3 {
        Some(bytes) => Ok(([(header::CONTENT_TYPE, "audio/mpeg")], bytes).into_response()),
        None => Err(err(StatusCode::SERVICE_UNAVAILABLE, "voice not configured")),
    }
}

// ---- speech-to-text ---------------------------------------------------------

/// Transcribe a recorded audio clip (ElevenLabs Scribe). The browser posts the raw
/// `MediaRecorder` blob as the request body with its `Content-Type` (WebM/Opus or WAV).
async fn stt(
    UserClaims(c): UserClaims,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiErr> {
    auth::require(&c, "assistant.use")?;
    if body.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "empty audio body"));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/webm")
        .to_string();
    let filename = if mime.contains("wav") {
        "audio.wav"
    } else {
        "audio.webm"
    }
    .to_string();
    let audio = body.to_vec();

    let text = blocking(move || app_core::listen::transcribe(&audio, &filename, &mime))
        .await?
        .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({ "text": text })))
}
