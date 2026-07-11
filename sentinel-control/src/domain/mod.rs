//! Tenant-scoped domain APIs (Phase 3): JWT + action-guarded HTTP handlers that
//! reuse app-core's tenant-scoped repos/services. The tenant ALWAYS comes from the
//! JWT (`claims.tid`), never the client — that is the isolation boundary. The repos
//! are sync (`Db::block`), so handlers run them on `spawn_blocking`.
use axum::http::StatusCode;
use axum::Router;
use uuid::Uuid;

use crate::auth::Claims;
use crate::AppState;

mod admin_providers;
mod agent;
mod alerts;
mod beacons;
mod billing;
mod boxes;
mod devices;
mod events;
mod jobs;
mod map;
mod persons;
mod platform_logs;
pub(crate) mod recordings;
mod workflows;

/// `(status, message)` error tuple — axum turns it into the HTTP response.
pub type ApiErr = (StatusCode, String);

/// Merge every domain sub-router. Each handler guards itself with `auth::require`.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(devices::router())
        .merge(agent::router())
        .merge(alerts::router())
        .merge(billing::router())
        .merge(persons::router())
        .merge(beacons::router())
        .merge(map::router())
        .merge(workflows::router())
        .merge(boxes::router())
        .merge(admin_providers::router())
        .merge(platform_logs::router())
        .merge(recordings::router())
        .merge(jobs::router())
        .merge(events::router())
}

/// The tenant this token operates in. The master's own (un-assumed) token carries no
/// tenant — it must `assume` one first (which mints a tenant-scoped token).
pub fn tenant_of(c: &Claims) -> Result<Uuid, ApiErr> {
    c.tenant()
        .ok_or((StatusCode::BAD_REQUEST, "no tenant context — assume a tenant first".into()))
}

/// Run a blocking (sync repo) closure off the async runtime.
pub async fn blocking<F, T>(f: F) -> Result<T, ApiErr>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("task panicked: {e}")))
}

/// A short helper for ad-hoc errors.
pub fn err(code: StatusCode, msg: impl Into<String>) -> ApiErr {
    (code, msg.into())
}
