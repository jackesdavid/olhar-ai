//! The agnostic event feed (card #66): `GET /events` with cursor pagination + filters, and
//! `GET /events/kinds` for the filter's catalog. Tenant-scoped — the tenant comes from the JWT.
use axum::extract::{Query, State};
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use app_core::events::{self, EventQuery};

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/events", get(list))
        .route("/events/kinds", get(kinds))
}

#[derive(Deserialize)]
struct ListQuery {
    kind:     Option<String>,
    source:   Option<String>,
    severity: Option<String>,
    camera:   Option<String>,
    from:     Option<String>,
    to:       Option<String>,
    /// Cursor: return events with id < after (newest-first paging).
    after:    Option<i64>,
    #[serde(default)]
    limit:    Option<i64>,
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<events::EventRow>>, ApiErr> {
    // The events feed is the successor to alerts; reuse its read permission until Phase 2 splits it.
    auth::require(&c, "alerts.read")?;
    let tid = tenant_of(&c)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = events::list_events(&st.pool, tid, EventQuery {
        kind: q.kind, source: q.source, severity: q.severity, camera: q.camera,
        from: q.from, to: q.to, after: q.after, limit,
    })
    .await
    .map_err(|e| { log::error!("list events: {e}"); err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database error") })?;
    Ok(Json(rows))
}

#[derive(serde::Serialize)]
struct KindOption { kind: &'static str, label: &'static str }

/// The kinds the feed can label — drives the filter dropdown from one source of truth.
async fn kinds(UserClaims(c): UserClaims) -> Result<Json<Vec<KindOption>>, ApiErr> {
    auth::require(&c, "alerts.read")?;
    Ok(Json(events::catalog().iter().map(|k| KindOption { kind: k.kind, label: k.label }).collect()))
}
