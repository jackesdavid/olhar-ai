//! Master-only routes for global notification provider configuration.
//! `PUT /admin/telegram-provider` sets the shared bot token used by ALL tenants;
//! `GET /admin/telegram-provider` returns the configured state (no token leak).
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};

use app_core::notification;

use super::{blocking, err, ApiErr};
use crate::auth::UserClaims;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/telegram-provider", get(get_provider).put(set_provider))
}

#[derive(Serialize)]
struct TelegramProviderState {
    /// The **global** shared bot is set. (Named for backward-compat; it is specifically the global
    /// token's state, not "does notification work at all" — that's what the count below clarifies.)
    configured: bool,
    bot_username: Option<String>,
    /// Tenants running their own bot token instead of the global one. Lets the UI avoid telling a
    /// master "Not configured" when delivery is in fact working per-tenant (card #73).
    tenant_bot_count: i64,
}

async fn get_provider(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<TelegramProviderState>, ApiErr> {
    if !c.master {
        return Err(err(StatusCode::FORBIDDEN, "master only"));
    }
    let db = st.db.clone();
    let state = blocking(move || {
        let (configured, bot_username) = notification::repo::load_global_telegram(&db)
            .map(|(token, username)| (!token.trim().is_empty(), username))
            .unwrap_or((false, None));
        TelegramProviderState {
            configured,
            bot_username,
            tenant_bot_count: notification::repo::count_tenant_telegram_bots(&db),
        }
    })
    .await?;
    Ok(Json(state))
}

#[derive(Deserialize)]
struct SetProviderReq {
    bot_token: String,
}

async fn set_provider(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<SetProviderReq>,
) -> Result<Json<TelegramProviderState>, ApiErr> {
    if !c.master {
        return Err(err(StatusCode::FORBIDDEN, "master only"));
    }
    let db = st.db.clone();
    let token = b.bot_token.trim().to_string();

    // Validate the token with Telegram BEFORE persisting it, and return the state we end up in.
    // Previously a bad token was saved silently with no username, so the operator had no way to tell
    // it never worked (the exact confusion card #73 is about). Now: an empty token clears the global
    // bot; a non-empty one must authenticate (`getMe`) or it's rejected and never stored.
    let outcome = blocking(move || -> Result<(Option<String>, i64), String> {
        if token.is_empty() {
            notification::repo::save_global_telegram(&db, "");
            return Ok((None, notification::repo::count_tenant_telegram_bots(&db)));
        }
        let cfg = notification::NotificationConfig { bot_token: token.clone(), pairing_code: String::new() };
        let username = notification::client::get_me(&cfg)?; // Err → token rejected, nothing saved
        notification::repo::save_global_telegram(&db, &token);
        notification::repo::save_global_telegram_username(&db, &username);
        Ok((Some(username), notification::repo::count_tenant_telegram_bots(&db)))
    })
    .await?;

    match outcome {
        Ok((bot_username, tenant_bot_count)) => Ok(Json(TelegramProviderState {
            configured: bot_username.is_some(),
            bot_username,
            tenant_bot_count,
        })),
        Err(e) => {
            log::warn!("global telegram token rejected: {e}");
            Err(err(StatusCode::BAD_REQUEST, "Telegram rejected that token — double-check it and try again."))
        }
    }
}
