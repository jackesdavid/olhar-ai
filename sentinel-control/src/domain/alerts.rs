//! Tenant-scoped alert log: list, stats, create, delete, and deliver-to-beacons.
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};

use app_core::alerts::{self, Alert, AlertLevel, AlertStore};
use app_core::notification::{NotificationConfig, NotificationStore};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/alerts", get(list).post(create))
        .route("/alerts/stats", get(stats))
        .route("/alerts/:id", axum::routing::delete(remove))
        .route("/alerts/:id/deliver", post(deliver))
}

/// The alert log, newest first.
async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<Alert>>, ApiErr> {
    auth::require(&c, "alerts.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let mut alerts = blocking(move || alerts::repo::load(&db, tid)).await?;
    alerts.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    Ok(Json(alerts))
}

#[derive(Serialize)]
struct AlertStats {
    total: u32,
    today: u32,
    by_level: Vec<(String, u32)>,
    undelivered: u32,
}

/// Aggregate counts for the dashboard stat cards.
async fn stats(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<AlertStats>, ApiErr> {
    auth::require(&c, "alerts.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let alerts = blocking(move || alerts::repo::load(&db, tid)).await?;
    let total = alerts.len() as u32;
    let today = alerts.iter().filter(|a| alerts::is_today(&a.created_at)).count() as u32;
    let undelivered = alerts.iter().filter(|a| a.deliveries.is_empty()).count() as u32;
    let by_level = AlertLevel::ALL
        .iter()
        .map(|&lvl| (lvl.label().to_string(), alerts.iter().filter(|a| a.level == lvl).count() as u32))
        .collect();
    Ok(Json(AlertStats { total, today, by_level, undelivered }))
}

#[derive(Deserialize)]
struct CreateReq {
    level: Option<String>,
    message: String,
    room: Option<String>,
    source: Option<String>,
}

/// Create an alert (level defaults to Info, source to "operator"), persist, return it.
async fn create(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<CreateReq>) -> Result<Json<Alert>, ApiErr> {
    auth::require(&c, "alerts.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let alert = blocking(move || {
        // Load the tenant's log into an ephemeral store so `create` assigns a correct
        // monotonic id, then it persists the new row to Postgres for us.
        let store: AlertStore = Arc::new(Mutex::new(alerts::repo::load(&db, tid)));
        let level = b.level.as_deref().and_then(AlertLevel::parse).unwrap_or_default();
        let source = b
            .source
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "operator".to_string());
        let room_name = b.room.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        alerts::create(&store, Some(&db), tid, level, b.message, None, room_name, source)
    })
    .await?;
    Ok(Json(alert))
}

/// Remove the alert with `id`. Returns 404 if it didn't exist.
async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u64>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "alerts.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let removed = blocking(move || {
        let store: AlertStore = Arc::new(Mutex::new(alerts::repo::load(&db, tid)));
        alerts::delete(&store, Some(&db), tid, id)
    })
    .await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "alert not found"))
    }
}

/// Deliver the alert with `id` to every enabled beacon (Telegram). Network IO.
async fn deliver(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<u64>) -> Result<Json<String>, ApiErr> {
    auth::require(&c, "alerts.ack")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let result = blocking(move || {
        let cfg = resolve_telegram(&db, tid).ok_or_else(|| {
            "Telegram is not configured (set the bot token in Beacons).".to_string()
        })?;
        let alert_store: AlertStore = Arc::new(Mutex::new(alerts::repo::load(&db, tid)));
        let notif_store: NotificationStore =
            Arc::new(Mutex::new(app_core::notification::repo::load_beacons(&db, tid)));
        alerts::deliver(&alert_store, Some(&db), tid, &notif_store, &cfg, id)
    })
    .await?;
    result.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

/// Resolve the tenant's active Telegram config: the saved row (if it has a token),
/// else the environment.
fn resolve_telegram(db: &app_core::db::Db, tid: uuid::Uuid) -> Option<NotificationConfig> {
    if let Some((bot_token, pairing_code)) = app_core::notification::repo::load_telegram(db, tid) {
        if !bot_token.trim().is_empty() {
            return Some(NotificationConfig { bot_token, pairing_code });
        }
    }
    NotificationConfig::from_env()
}
