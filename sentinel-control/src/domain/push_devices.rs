//! Mobile push devices (card #139, Phase 1) — the Recipient delivery channel parallel to the
//! Telegram beacons in `domain/beacons.rs`. The app registers its Expo push token here after
//! login; `sentinel-notification` fans a `channel: "push"` job out to every enabled device.
//!
//! Gated on the same `beacons.*` permissions as Telegram recipients: registering a phone IS
//! managing a recipient, so it should not need a new permission in the RBAC catalog.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use app_core::notification::push::{self, PushDevice};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/push/devices", get(list).post(register))
        .route("/push/devices/:id", axum::routing::delete(remove))
        .route("/push/devices/test", post(test_send))
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<PushDevice>>, ApiErr> {
    auth::require(&c, "beacons.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    Ok(Json(blocking(move || push::list(&db, tid)).await?))
}

#[derive(Deserialize)]
struct RegisterReq {
    /// The `ExponentPushToken[…]` string from `expo-notifications`.
    device_token: String,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    label: Option<String>,
}

/// Register (or refresh) this phone. Idempotent per `(tenant, device_token)` so an app reinstall
/// or token rotation updates in place instead of duplicating pushes to one device.
async fn register(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<RegisterReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let token = b.device_token.trim().to_string();
    if token.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "device_token is required"));
    }
    let user_id = c.sub.parse().ok();
    let platform = b.platform.unwrap_or_else(|| "unknown".into());
    let label = b.label.unwrap_or_default();
    let db = st.db.clone();
    let id = blocking(move || push::register(&db, tid, user_id, &token, &platform, &label))
        .await?
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn remove(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let gone = blocking(move || push::delete(&db, tid, id)).await?;
    if gone {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "no such device"))
    }
}

#[derive(Deserialize)]
struct TestReq {
    #[serde(default)]
    label: Option<String>,
}

/// Send a test push — the phone equivalent of the beacon "test" button, so an operator can prove
/// delivery without waiting for a workflow to fire.
async fn test_send(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<TestReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "beacons.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let sent = blocking(move || {
        let tokens = push::enabled_tokens(&db, tid, b.label.as_deref());
        push::send(
            &tokens,
            "Olhar AI",
            "Test notification — your phone is set up.",
        )
    })
    .await?
    .map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(serde_json::json!({ "sent": sent })))
}
