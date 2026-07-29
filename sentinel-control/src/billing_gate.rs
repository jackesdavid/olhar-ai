//! Soft-lock: a tenant whose subscription lapsed (`expired` / `past_due` / `canceled`)
//! goes **read-only** — it keeps seeing its data, but every mutation is refused with
//! `402 Payment Required` and a prompt to pick a plan. Not a hard lockout.
//!
//! One choke point (an axum layer on the domain router) instead of a check sprinkled over
//! every write handler. Reads (`GET`/`HEAD`/`OPTIONS`) always pass, and the billing +
//! webhook routes are exempt — otherwise a locked tenant could never pay to get out.
//!
//! The status is resolved at **read time** (`subscriptions::current` folds in a lapsed
//! trial), so there is no window where an expired trial still has write access while the
//! hourly sweep hasn't run yet.

use axum::extract::{Request, State};
use axum::http::{header::AUTHORIZATION, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use app_core::billing::subscriptions;

use crate::AppState;

/// Paths a soft-locked tenant may still write to — paying is how it gets unlocked, and
/// provider webhooks must always land.
fn exempt(path: &str) -> bool {
    path.starts_with("/billing") || path.starts_with("/webhooks") || path.starts_with("/auth")
}

pub async fn soft_lock(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let is_read = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if is_read || exempt(req.uri().path()) {
        return next.run(req).await;
    }

    // Unauthenticated or master → let the handler's own extractor decide.
    let Some(claims) = bearer_claims(&st, &req) else {
        return next.run(req).await;
    };
    if claims.master {
        return next.run(req).await;
    }
    let Some(tid) = claims.tid.as_deref().and_then(|s| Uuid::parse_str(s).ok()) else {
        return next.run(req).await;
    };

    let db = st.db.clone();
    let status = tokio::task::spawn_blocking(move || subscriptions::effective_status(&db, tid))
        .await
        .ok()
        .flatten();

    match status {
        Some(s) if subscriptions::LOCKED_STATUSES.contains(&s.as_str()) => (
            StatusCode::PAYMENT_REQUIRED,
            format!("Your subscription is {s} — Sentinel is read-only until you choose a plan."),
        )
            .into_response(),
        // No subscription at all (a tenant created before billing) stays unlocked.
        _ => next.run(req).await,
    }
}

fn bearer_claims(st: &AppState, req: &Request) -> Option<crate::auth::Claims> {
    let raw = req.headers().get(AUTHORIZATION)?.to_str().ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    crate::auth::verify(&st.jwt_secret, token).ok()
}
