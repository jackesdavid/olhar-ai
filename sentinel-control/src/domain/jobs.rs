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
        .route("/admin/jobs/series", get(series))
        .route("/admin/jobs", get(list))
}

/// Master guard: this console is cross-tenant, so only the SaaS operator sees it.
fn require_master(c: &crate::auth::Claims) -> Result<(), ApiErr> {
    if c.master {
        Ok(())
    } else {
        Err(err(axum::http::StatusCode::FORBIDDEN, "master only"))
    }
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
async fn stats(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(qq): Query<StatsQuery>,
) -> Result<Json<Vec<work_queue::CategoryStat>>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let tenant = qq.tenant;
    let window = window_of(qq.window);
    Ok(Json(
        blocking(move || work_queue::monitor_stats_blocking(&db, tenant, window)).await?,
    ))
}

/// Bucket size for the time series: aim for ~120 points, rounded up to a whole minute (min 60s).
/// So a 1-hour window is per-minute; wider windows coarsen automatically instead of returning
/// thousands of points.
fn bucket_for(window_secs: i64) -> i64 {
    // Aim for ~40 buckets — few enough that a value label fits above every bar, rounded up to a
    // whole minute. Sub-5-min windows (the "last minute" view) keep 5-second buckets.
    if window_secs <= 300 {
        return 5;
    }
    let raw = (window_secs / 40).max(60);
    ((raw + 59) / 60) * 60
}

#[derive(serde::Serialize)]
struct SeriesResponse {
    /// Bucket width in seconds, so the client knows the axis step and can zero-fill.
    bucket_secs: i64,
    /// `(bucket_start_epoch, count)` — only non-empty buckets.
    points: Vec<SeriesPoint>,
}
#[derive(serde::Serialize)]
struct SeriesPoint {
    t: i64,
    c: i64,
}

/// Jobs created per time bucket within the window — the load-over-time chart.
async fn series(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(qq): Query<StatsQuery>,
) -> Result<Json<SeriesResponse>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let tenant = qq.tenant;
    let window = window_of(qq.window);
    let bucket = bucket_for(window);
    let rows =
        blocking(move || work_queue::monitor_series_blocking(&db, tenant, window, bucket)).await?;
    let points = rows
        .into_iter()
        .map(|(t, c)| SeriesPoint { t, c })
        .collect();
    Ok(Json(SeriesResponse {
        bucket_secs: bucket,
        points,
    }))
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
async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(qq): Query<ListQuery>,
) -> Result<Json<ListResponse>, ApiErr> {
    require_master(&c)?;
    let db = st.db.clone();
    let limit = qq.limit.unwrap_or(50).clamp(1, 200);
    let offset = qq.offset.unwrap_or(0).max(0);
    let window = window_of(qq.window);
    let (tenant, category, status) = (qq.tenant, qq.category, qq.status);
    let (total, items) = blocking(move || {
        work_queue::monitor_list_blocking(
            &db,
            tenant,
            category.as_deref(),
            status.as_deref(),
            window,
            limit,
            offset,
        )
    })
    .await?;
    Ok(Json(ListResponse { total, items }))
}
