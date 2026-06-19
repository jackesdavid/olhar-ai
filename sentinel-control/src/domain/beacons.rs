//! Tenant-scoped notification beacons (Telegram targets) + the Telegram config.
//! The pairing poller itself runs in the edge/flow tier; the cloud manages the rows.
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};

use app_core::notification::{self, NotificationConfig, NotificationStore, NotificationTarget};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/beacons", get(list))
        .route("/beacons/:chat_id/toggle", post(toggle))
        .route("/beacons/:chat_id/test", post(test_send))
        .route("/beacons/:chat_id", axum::routing::patch(rename).delete(remove))
        .route("/telegram", get(get_config).put(set_config))
}

async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<NotificationTarget>>, ApiErr> {
    auth::require(&c, "beacons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || notification::repo::load_beacons(&db, tid)).await?))
}

#[derive(Deserialize)]
struct ToggleReq {
    enabled: bool,
}

async fn toggle(UserClaims(c): UserClaims, State(st): State<AppState>, Path(chat_id): Path<i64>, Json(b): Json<ToggleReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let found = blocking(move || {
        let store: NotificationStore = Arc::new(Mutex::new(notification::repo::load_beacons(&db, tid)));
        notification::set_enabled_by_id(&store, Some(&db), tid, chat_id, b.enabled).is_some()
    })
    .await?;
    if found { Ok(StatusCode::NO_CONTENT) } else { Err(err(StatusCode::NOT_FOUND, "Beacon not found.")) }
}

#[derive(Deserialize)]
struct RenameReq {
    label: String,
}

async fn rename(UserClaims(c): UserClaims, State(st): State<AppState>, Path(chat_id): Path<i64>, Json(b): Json<RenameReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || {
        let store: NotificationStore = Arc::new(Mutex::new(notification::repo::load_beacons(&db, tid)));
        notification::rename_target(&store, Some(&db), tid, chat_id, &b.label)
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(chat_id): Path<i64>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let removed = blocking(move || {
        let store: NotificationStore = Arc::new(Mutex::new(notification::repo::load_beacons(&db, tid)));
        notification::delete_target(&store, Some(&db), tid, chat_id).is_some()
    })
    .await?;
    if removed { Ok(StatusCode::NO_CONTENT) } else { Err(err(StatusCode::NOT_FOUND, "Beacon not found.")) }
}

#[derive(Serialize)]
struct TelegramState {
    configured: bool,
    pairing_code: String,
    bot_username: Option<String>,
}

/// Telegram config WITHOUT leaking the bot token.
async fn get_config(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<TelegramState>, ApiErr> {
    auth::require(&c, "beacons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let state = blocking(move || {
        notification::repo::load_telegram_full(&db, tid)
            .map(|(bot_token, pairing_code, bot_username)| TelegramState {
                configured: !bot_token.trim().is_empty(),
                pairing_code,
                bot_username,
            })
            .unwrap_or(TelegramState { configured: false, pairing_code: String::new(), bot_username: None })
    })
    .await?;
    Ok(Json(state))
}

#[derive(Deserialize)]
struct SetTelegramReq {
    /// Blank means "keep the saved token" (the token is never sent to the UI).
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    pairing_code: String,
}

async fn set_config(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<SetTelegramReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || {
        let token = {
            let t = b.bot_token.trim();
            if t.is_empty() {
                notification::repo::load_telegram(&db, tid).map(|(tok, _)| tok).unwrap_or_default()
            } else {
                t.to_string()
            }
        };
        notification::repo::save_telegram(&db, tid, &token, b.pairing_code.trim());
        // Resolve bot username for the deep-link (best-effort; ignored on failure).
        if !token.trim().is_empty() {
            let cfg = NotificationConfig { bot_token: token, pairing_code: String::new() };
            if let Ok(username) = notification::client::get_me(&cfg) {
                notification::repo::save_telegram_username(&db, tid, &username);
            }
        }
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_send(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(chat_id): Path<i64>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || {
        let cfg = notification::repo::load_telegram(&db, tid)
            .and_then(|(token, code)| {
                if token.trim().is_empty() {
                    None
                } else {
                    Some(NotificationConfig { bot_token: token, pairing_code: code })
                }
            })
            .ok_or_else(|| "Telegram is not configured.".to_string())?;
        notification::client::send_message(&cfg, chat_id, "Sentinel test \u{2713} — delivery confirmed.")
            .map_err(|e| format!("Send failed: {e}"))
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(StatusCode::NO_CONTENT)
}
