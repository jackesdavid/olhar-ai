//! Tenant-scoped notification beacons (Telegram targets) + the Telegram config.
//! Bot token is now a master-level global setting (see domain/admin_providers.rs).
//! Tenants configure only their pairing code; the shared bot is used for delivery.
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
    // The notification queue view moved to the master load panel (/admin/jobs) — it is
    // cross-tenant system load, not a tenant-facing screen.
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
    /// True if the global Telegram provider (or a tenant-own fallback) has a bot token set.
    provider_configured: bool,
    pairing_code: String,
    bot_username: Option<String>,
}

/// Telegram state without leaking any tokens.
async fn get_config(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<TelegramState>, ApiErr> {
    auth::require(&c, "beacons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let state = blocking(move || {
        let (pairing_code, mut bot_username, provider_configured) =
            notification::repo::load_tenant_notification_state(&db, tid);
        // The enrollment link/QR need the bot's @username. If a token is set but the username was
        // never resolved (card #73), fill it now (one getMe, then cached) so the link can render.
        if provider_configured && bot_username.as_deref().unwrap_or("").trim().is_empty() {
            bot_username = notification::repo::ensure_bot_username(&db, tid);
        }
        TelegramState { provider_configured, pairing_code, bot_username }
    })
    .await?;
    Ok(Json(state))
}

#[derive(Deserialize)]
struct SetTelegramReq {
    #[serde(default)]
    pairing_code: String,
}

async fn set_config(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<SetTelegramReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    blocking(move || {
        notification::repo::save_pairing_code(&db, tid, b.pairing_code.trim());
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
        // Resolve effective bot token: tenant-own first, then global, then env.
        let token = notification::repo::load_telegram(&db, tid)
            .map(|(tok, _)| tok)
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                notification::repo::load_global_telegram(&db)
                    .map(|(t, _)| t)
                    .filter(|t| !t.trim().is_empty())
            })
            .or_else(|| notification::NotificationConfig::from_env().map(|c| c.bot_token))
            .ok_or_else(|| "Telegram provider not configured.".to_string())?;

        let cfg = NotificationConfig { bot_token: token, pairing_code: String::new() };
        notification::client::send_message(&cfg, chat_id, "Sentinel test \u{2713} — delivery confirmed.")
            .map_err(|e| format!("Send failed: {e}"))
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(StatusCode::NO_CONTENT)
}
