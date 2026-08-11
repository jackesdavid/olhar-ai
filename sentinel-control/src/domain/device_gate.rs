//! Device Gate answers (card #176) — the two endpoints a phone talks to.
//!
//! `POST /device-gate/:id/answer` settles a request; `GET /device-gate/pending` lists what is still
//! waiting on the calling device. The second is not a fallback: decision 05 established that a push
//! can be batched, silenced or lost, so a request that exists only inside a notification is a
//! request that quietly expires. The app fetches this on launch and on foreground, and the push is
//! the optimisation.
//!
//! **Neither handler calls `auth::require`, deliberately.** Answering a gate is not an
//! administrative action gated by a role — it is the addressed person confirming something on their
//! own phone. Requiring a permission would mean a Viewer holding the phone the graph addressed
//! could not open the door the graph asked them about, which is the opposite of what the graph
//! author wrote. What guards these is OWNERSHIP: the request points at one `push_devices` row
//! (decision 01), and the caller's session must own that row. Two proofs, neither sufficient alone
//! — the token proves WHICH request, the session proves WHO.
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::json;

use app_core::workflows::device_gate::{self, AuthFactor, PendingRequest};

use super::{blocking, err, tenant_of, ApiErr};
use crate::auth::UserClaims;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/device-gate/pending", get(pending))
        .route("/device-gate/:id/answer", post(answer))
}

#[derive(Deserialize)]
struct PendingQuery {
    /// The `push_devices` row this phone registered as (`POST /push/devices` returns the id).
    device_id: uuid::Uuid,
}

/// What this device is still being asked. Scoped to the CALLING DEVICE, not the user's other
/// phones: only the addressed device may answer, so listing another device's requests would only
/// show someone a slider that cannot work.
async fn pending(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Query(q): Query<PendingQuery>,
) -> Result<Json<Vec<PendingRequest>>, ApiErr> {
    let tid = tenant_of(&c)?;
    let user_id: Option<uuid::Uuid> = c.sub.parse().ok();
    let db = st.db.clone();
    let device_id = q.device_id;

    let owns = blocking(move || device_gate::owner_of_device(&db, device_id))
        .await?
        .is_some_and(|(owner, dev_tenant)| dev_tenant == tid && owner.is_some() && owner == user_id);
    // 404 rather than 403 for someone else's device: a caller who does not own it has no business
    // learning whether the id exists.
    if !owns {
        return Err(err(StatusCode::NOT_FOUND, "no such device"));
    }

    let db = st.db.clone();
    Ok(Json(
        blocking(move || device_gate::pending_for_device(&db, device_id)).await?,
    ))
}

#[derive(Deserialize)]
struct AnswerReq {
    /// The single-use token from the push payload. Proves WHICH request is being answered; the
    /// session proves who is answering.
    token: String,
    approved: bool,
    /// `biometric` | `passcode` — what the phone actually used (decision 02). REQUIRED: a
    /// fallback to the passcode is acceptable, an approval whose strength is unknowable is not,
    /// and a default here would be that unknowable approval wearing a confident label.
    factor: String,
}

async fn answer(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(b): Json<AnswerReq>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let tid = tenant_of(&c)?;
    let user_id: Option<uuid::Uuid> = c.sub.parse().ok();
    let Some(factor) = AuthFactor::from_str(b.factor.trim()) else {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "factor must be 'biometric' or 'passcode'",
        ));
    };
    if b.token.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "token is required"));
    }

    let db = st.db.clone();
    let owner = blocking(move || device_gate::owner_of_request(&db, id)).await?;
    // Unknown request, another tenant's, or a device this session does not own — all one answer.
    // Distinguishing them would turn this endpoint into an oracle for which request ids exist.
    let Some((device_owner, req_tenant, _)) = owner else {
        return Err(err(StatusCode::NOT_FOUND, "no such request"));
    };
    if req_tenant != tid || device_owner.is_none() || device_owner != user_id {
        return Err(err(StatusCode::NOT_FOUND, "no such request"));
    }

    let db = st.db.clone();
    let token = b.token.trim().to_string();
    let approved = b.approved;
    let settled = blocking(move || {
        device_gate::settle_with_answer(&db, id, &token, approved, factor, user_id)
    })
    .await?;

    let Some(target) = settled else {
        // Already answered, expired, or the token was wrong. ONE response for all three: a caller
        // who guessed a token must not learn that the request was real and merely finished.
        return Err(err(
            StatusCode::CONFLICT,
            "this request can no longer be answered",
        ));
    };

    // Wake the sweep so the graph continues in a moment rather than within its poll interval. This
    // is a LATENCY optimisation and nothing more: the row carries `resumed_at IS NULL`, so
    // `gate_consumer` delivers the answer with or without this. NOTIFY rather than an HTTP POST
    // because `FLOW_URL` is load-balanced DNS — a POST reaches one replica, and if that is not the
    // one that sweeps first, the nudge was wasted anyway.
    let db = st.db.clone();
    let _ = blocking(move || db.notify("device_gate_settled", "")).await;

    Ok(Json(json!({
        "state": target.state.as_str(),
        "arm": target.state.exec_arm(),
    })))
}
