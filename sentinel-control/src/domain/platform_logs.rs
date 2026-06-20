//! Tenant-scoped unified audit log: `GET /platform-logs` with cursor-based
//! pagination and multi-dimensional filtering. All modules write here; this
//! handler is the single read path the UI consumes.
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::platform_logs::{self, LogQuery};

use super::{tenant_of, ApiErr};
use crate::auth::UserClaims;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/platform-logs", get(list))
}

#[derive(Deserialize)]
struct ListQuery {
    module:      Option<String>,
    trace_id:    Option<Uuid>,
    entity_type: Option<String>,
    entity_id:   Option<String>,
    level:       Option<String>,
    from:        Option<String>,
    to:          Option<String>,
    /// Cursor: return entries with id < after (oldest page boundary, newest-first).
    after:       Option<i64>,
    #[serde(default)]
    limit:       Option<i64>,
}

async fn list(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiErr> {
    // Any authenticated user can read logs; tighten to "logs.read" if needed later.
    let tid = tenant_of(&c)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let rows = platform_logs::list_logs(
        &st.pool,
        tid,
        LogQuery {
            module:      q.module,
            trace_id:    q.trace_id,
            entity_type: q.entity_type,
            entity_id:   q.entity_id,
            level:       q.level,
            from:        q.from,
            to:          q.to,
            after:       q.after,
            limit,
        },
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("query failed: {e}")))?;

    let next_cursor = rows.last().map(|r| r.id);
    let count = rows.len();
    Ok(Json(serde_json::json!({
        "items": rows,
        "next_cursor": next_cursor,
        "total_in_page": count,
    })))
}
