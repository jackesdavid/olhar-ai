//! Tenant-scoped alert log — now a thin view over the agnostic event stream (`kind = 'alert'`,
//! card #66). The `alerts` table is gone; these endpoints read/write `events` and map to the
//! shape the /alerts page already expects. Producers everywhere emit alert events; this is the
//! CRUD + deliver surface for the operator-facing screen.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use app_core::events::{self, AlertLevel, EventRow, NewEvent};
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

/// The alert shape the frontend consumes — mapped from an alert event.
#[derive(Serialize)]
struct AlertView {
    id: i64,
    level: String,   // severity string (debug|info|warn|error), lowercase
    message: String,
    room_id: Option<i64>,
    room_name: Option<String>,
    source: String,
    created_at: String,
    deliveries: Vec<Value>,
}

fn to_view(e: EventRow) -> AlertView {
    let deliveries = e.data.as_ref()
        .and_then(|d| d.get("deliveries"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    AlertView {
        id: e.id,
        level: e.severity,
        message: e.message.unwrap_or_default(),
        room_id: e.room_id,
        room_name: e.room_name,
        source: e.source,
        created_at: e.occurred_at,
        deliveries,
    }
}

/// The alert log, newest first.
async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<AlertView>>, ApiErr> {
    auth::require(&c, "alerts.read")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let rows = blocking(move || events::list_kind_sync(&db, tid, "alert", None, 1000)).await?;
    Ok(Json(rows.into_iter().map(to_view).collect()))
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
    let rows = blocking(move || events::list_kind_sync(&db, tid, "alert", None, 1000)).await?;
    let total = rows.len() as u32;
    let today = rows.iter().filter(|e| app_core::timeutil::is_today(&e.occurred_at)).count() as u32;
    let undelivered = rows.iter().filter(|e| !events::event_delivered(e)).count() as u32;
    let by_level = AlertLevel::ALL
        .iter()
        .map(|&lvl| (lvl.label().to_string(), rows.iter().filter(|e| AlertLevel::parse(&e.severity) == Some(lvl)).count() as u32))
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

/// Create an alert event (level defaults to Info, source to "operator"), return it.
async fn create(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<CreateReq>) -> Result<Json<AlertView>, ApiErr> {
    auth::require(&c, "alerts.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let view = blocking(move || -> Result<AlertView, String> {
        let level = b.level.as_deref().and_then(AlertLevel::parse).unwrap_or_default();
        let source = b.source.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "operator".to_string());
        let room_name = b.room.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        let mut ev = NewEvent::new(tid, "alert", source);
        ev.severity = level.severity().to_string();
        ev.message = Some(b.message);
        ev.room_name = room_name;
        let id = events::insert_event_returning_sync(&db, ev).ok_or("failed to record alert")?;
        events::event_by_id_sync(&db, tid, id).map(to_view).ok_or("alert vanished after insert".into())
    })
    .await?
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(view))
}

/// Remove the alert event with `id`. Returns 404 if it didn't exist.
async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<i64>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "alerts.write")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let removed = blocking(move || events::delete_event_sync(&db, tid, id)).await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "alert not found"))
    }
}

/// Deliver the alert event with `id` to every enabled beacon (Telegram). Network IO.
async fn deliver(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<String>, ApiErr> {
    auth::require(&c, "alerts.ack")?;
    let tid = tenant_of(&c)?;
    let db = st.db.clone();
    let result = blocking(move || {
        let cfg = resolve_telegram(&db, tid).ok_or_else(|| {
            "Telegram is not configured (set the bot token in Beacons).".to_string()
        })?;
        let notif_store: NotificationStore =
            std::sync::Arc::new(std::sync::Mutex::new(app_core::notification::repo::load_beacons(&db, tid)));
        events::deliver_event_to_beacons(&db, tid, id, &notif_store, &cfg)
    })
    .await?;
    result.map(Json).map_err(|e| err(StatusCode::BAD_REQUEST, e))
}

/// Resolve the tenant's active Telegram config: the saved row (if it has a token), else env.
fn resolve_telegram(db: &app_core::db::Db, tid: uuid::Uuid) -> Option<NotificationConfig> {
    if let Some((bot_token, pairing_code)) = app_core::notification::repo::load_telegram(db, tid) {
        if !bot_token.trim().is_empty() {
            return Some(NotificationConfig { bot_token, pairing_code });
        }
    }
    NotificationConfig::from_env()
}
