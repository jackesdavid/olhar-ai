//! Master-only view of the unified async work queue (`work_jobs`) — system load across every
//! tenant, or one. Not tenant-facing: an operator reads it to see what's queued, running, done,
//! and failing across clip-delete, workflow events, and notification sends, all in one place.

use axum::extract::{Query, State};
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::work_queue;

use super::{blocking, err, ApiErr};
use crate::auth::UserClaims;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/jobs/stats", get(stats))
        .route("/admin/jobs", get(list))
}

/// Master guard: this console is cross-tenant, so only the SaaS operator sees it.
fn require_master(c: &crate::auth::Claims) -> Result<(), ApiErr> {
    if c.master { Ok(()) } else { Err(err(axum::http::StatusCode::FORBIDDEN, "master only")) }
}

/// Default look-back when the client doesn't ask: one day. The panel is about the recent past.
const DEFAULT_WINDOW_SECS: i64 = 86_400;
/// Clamp: at least a minute, at most 30 days — the queue is UNLOGGED and short-lived anyway.
fn window_of(w: Option<i64>) -> i64 {
    w.unwrap_or(DEFAULT_WINDOW_SECS).clamp(60, 30 * 86_400)
}

#[derive(Deserialize)]
struct StatsQuery {
    /// Narrow to one tenant; omit for the whole system.
    tenant: Option<Uuid>,
    /// Look-back window in seconds (default 1 day).
    window: Option<i64>,
}

/// Job counts by (category, status) within the window — the load overview.
async fn stats(UserClaims(c): UserClaims, State(st): State<AppState>, Query(qq): Query<StatsQuery>) -> Result<Json<Vec<work_queue::CategoryStat>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let tenant = qq.tenant;
    let window = window_of(qq.window);
    Ok(Json(blocking(move || work_queue::monitor_stats_blocking(&db, tenant, window)).await?))
}

#[derive(Deserialize)]
struct ListQuery {
    tenant: Option<Uuid>,
    category: Option<String>,
    status: Option<String>,
    window: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(serde::Serialize)]
struct ListResponse {
    total: i64,
    items: Vec<work_queue::MonitorJob>,
}

/// Recent jobs, filterable by tenant / category / status.
async fn list(UserClaims(c): UserClaims, State(st): State<AppState>, Query(qq): Query<ListQuery>) -> Result<Json<ListResponse>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let limit = qq.limit.unwrap_or(50).clamp(1, 200);
    let offset = qq.offset.unwrap_or(0).max(0);
    let window = window_of(qq.window);
    let (tenant, category, status) = (qq.tenant, qq.category, qq.status);
    let (total, items) = blocking(move || {
        work_queue::monitor_list_blocking(&db, tenant, category.as_deref(), status.as_deref(), window, limit, offset)
    })
    .await?;
    Ok(Json(ListResponse { total, items }))
}
