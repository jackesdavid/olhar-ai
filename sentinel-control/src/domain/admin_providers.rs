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
    configured: bool,
    bot_username: Option<String>,
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
        notification::repo::load_global_telegram(&db)
            .map(|(token, username)| TelegramProviderState {
                configured: !token.trim().is_empty(),
                bot_username: username,
            })
            .unwrap_or(TelegramProviderState { configured: false, bot_username: None })
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
) -> Result<StatusCode, ApiErr> {
    if !c.master {
        return Err(err(StatusCode::FORBIDDEN, "master only"));
    }
    let db = st.db.clone();
    blocking(move || {
        let token = b.bot_token.trim().to_string();
        notification::repo::save_global_telegram(&db, &token);
        if !token.is_empty() {
            let cfg = notification::NotificationConfig {
                bot_token: token,
                pairing_code: String::new(),
            };
            if let Ok(username) = notification::client::get_me(&cfg) {
                notification::repo::save_global_telegram_username(&db, &username);
            }
        }
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
