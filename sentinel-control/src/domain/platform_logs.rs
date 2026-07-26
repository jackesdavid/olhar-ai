//! Tenant-scoped unified audit log: `GET /platform-logs` with cursor-based
//! pagination and multi-dimensional filtering. All modules write here; this
//! handler is the single read path the UI consumes.
//!
//! Card #146 also hosts `POST /logs/client` here — the browser's global error handler ships
//! `window.onerror`/unhandledrejection here (session-authed) and control forwards them to Datadog as
//! `service:app`, tenant-tagged. The DD key stays server-side (no Browser-Logs SDK key in the bundle).
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

use app_core::platform_logs::{self, LogQuery, PlatformLog};

use super::{tenant_of, ApiErr};
use crate::auth::UserClaims;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/platform-logs", get(list))
        .route("/logs/client", post(client_log))
}

/// A frontend error/log report. `message` + `level` shape the record; every other field (`stack`,
/// `source`, `line`, `component`, …) is spread into the Datadog record as a searchable attribute.
#[derive(Deserialize)]
struct ClientLog {
    #[serde(default)]
    level: String,
    #[serde(default)]
    message: String,
    /// The page URL the error fired on.
    #[serde(default)]
    url: Option<String>,
    /// Anything else the client attached (stack, source, line, col, component, …).
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Ship a browser error to Datadog as `service:app` (module `app`), tenant-scoped. Fire-and-forget:
/// the log queue ships to Datadog when configured (else Postgres) and drops on backpressure, so a
/// misbehaving tab can never stall control. Errors here are swallowed — telemetry must not 500.
async fn client_log(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ClientLog>,
) -> Result<impl IntoResponse, ApiErr> {
    let tid = tenant_of(&c)?;
    let level = match body.level.to_lowercase().as_str() {
        "error" => "error",
        "warn" => "warn",
        "debug" => "debug",
        _ => "info",
    };
    let mut payload = body.extra;
    if let Some(u) = &body.url {
        payload.insert("url".into(), serde_json::json!(u));
    }
    if let Some(ua) = headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()) {
        payload.insert("user_agent".into(), serde_json::json!(ua));
    }
    st.log_queue.push(PlatformLog {
        tenant_id: tid,
        module: "app",
        trace_id: None,
        entity_id: None,
        entity_type: Some("client_error"),
        level,
        message: if body.message.is_empty() { "client error".into() } else { body.message },
        payload: Some(serde_json::Value::Object(payload)),
    });
    Ok(StatusCode::NO_CONTENT)
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
