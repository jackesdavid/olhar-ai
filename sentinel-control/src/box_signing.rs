//! Verification side of edge → control request signing (see [`app_core::signing`]).
//!
//! Two pieces: a bootstrap endpoint that hands a box its key, and a middleware that checks the
//! signature on `/config/*`.
//!
//! ### The three outcomes, and why there are three
//!
//! | presented | result |
//! |---|---|
//! | no signature headers | pass through; the box's `signing_mode` decides whether that is allowed |
//! | headers present, invalid | **401, always, in every mode** |
//! | headers present, valid | verified; identity attached as [`VerifiedBox`] |
//!
//! The middle row is what keeps this from becoming a fourth fail-open bug. This codebase already
//! had three (`JWT_SECRET` defaulting to a literal, and both payment webhooks accepting anything
//! when their secret was unset), and every one of them keyed off "is this env var set". Here a
//! wrong signature is never equivalent to no signature; the only thing policy controls is whether
//! *absence* is tolerated, and that is a per-box row in the database, not an environment variable.
//!
//! Rollout depends on it: appliances in the field today sign nothing, and control must keep serving
//! them while they update. `signing_mode` moves one box at a time.

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use app_core::signing::{self, Canonical};
use app_core::timeutil::now_rfc3339;

use crate::AppState;

/// Largest body we will buffer to hash.
///
/// Explicit, because the alternative — buffering whatever arrives — is a memory-exhaustion DoS on
/// an endpoint reachable by anything holding a box token. Set above the effective limit the `Json`
/// extractor already imposes, so this layer never rejects a request the handler would have
/// accepted; it exists to bound the pathological case, not to be the body policy.
const MAX_SIGNED_BODY: usize = 4 * 1024 * 1024;

/// A request whose signature verified. Attached as a request extension.
///
/// Nothing reads it yet, and that is deliberate rather than an oversight: the handlers still
/// resolve their box through `box_tenant`/`box_identity`, which take a `&HeaderMap` and so cannot
/// see extensions. Moving them onto this is the follow-up that removes the duplicate token lookup
/// a signed request currently pays. Kept here because it is the handoff point that change needs,
/// and because the identity it carries is the verified one — derived from the key, not asserted by
/// the caller.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct VerifiedBox {
    pub box_id: Uuid,
    pub tenant_id: Uuid,
    pub key_id: Uuid,
}

/// The signing headers, when a caller presents them.
struct Presented {
    key_id: Uuid,
    timestamp: i64,
    nonce: String,
    signature: String,
}

const H_KEY_ID: &str = "x-sentinel-key-id";
const H_TIMESTAMP: &str = "x-sentinel-timestamp";
const H_NONCE: &str = "x-sentinel-nonce";
const H_SIGNATURE: &str = "x-sentinel-signature";

/// `None` = the caller sent no signing headers at all. `Some(Err(()))` = it sent some but they
/// don't parse, which is a refusal rather than an unsigned request: a half-present signature is a
/// broken client or a probe, never something to wave through.
fn presented(h: &HeaderMap) -> Option<Result<Presented, ()>> {
    let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::trim);
    let (key_id, ts, nonce, sig) = (
        get(H_KEY_ID),
        get(H_TIMESTAMP),
        get(H_NONCE),
        get(H_SIGNATURE),
    );
    if key_id.is_none() && ts.is_none() && nonce.is_none() && sig.is_none() {
        return None;
    }
    let parsed = (|| {
        Some(Presented {
            key_id: key_id?.parse().ok()?,
            timestamp: ts?.parse().ok()?,
            nonce: nonce?.to_string(),
            signature: sig?.to_string(),
        })
    })();
    Some(parsed.ok_or(()))
}

fn refuse(code: &str, msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": code, "message": msg })),
    )
        .into_response()
}

/// Verify the signature on a `/config/*` request.
///
/// Runs before the handler's extractors, because it is the only place the raw body still exists —
/// `box_tenant`/`box_identity` receive only a `&HeaderMap` and could never hash it. The body is
/// buffered and then put back, so downstream `Json<T>` sees an ordinary complete body.
pub async fn verify_signature(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();

    let Some(parsed) = presented(&parts.headers) else {
        // Unsigned. Allowed only while this box is still on 'off'.
        return match unsigned_allowed(&st, &parts.headers).await {
            Ok(()) => next.run(Request::from_parts(parts, body)).await,
            Err(resp) => resp,
        };
    };
    let Ok(p) = parsed else {
        return refuse(
            "malformed_signature",
            "signing headers incomplete or unparseable",
        );
    };

    // GET/HEAD carry no body, and one of them is the long-lived SSE stream — buffering that would
    // hang until the stream ended, i.e. forever.
    let has_body = !matches!(
        parts.method,
        axum::http::Method::GET | axum::http::Method::HEAD
    );
    let bytes = if has_body {
        match axum::body::to_bytes(body, MAX_SIGNED_BODY).await {
            Ok(b) => b,
            Err(_) => {
                return (StatusCode::PAYLOAD_TOO_LARGE, "body too large to sign").into_response()
            }
        }
    } else {
        Bytes::new()
    };

    let key = match live_key(&st, p.key_id).await {
        Ok(Some(k)) => k,
        // Unknown, revoked, or expired all answer the same way: a probe must not learn which.
        Ok(None) => return refuse("bad_signature", "signature rejected"),
        Err(resp) => return resp,
    };

    let canonical = Canonical {
        method: parts.method.as_str(),
        path: parts.uri.path(),
        query: parts.uri.query().unwrap_or(""),
        timestamp: p.timestamp,
        nonce: &p.nonce,
        key_id: &p.key_id.to_string(),
        body: &bytes,
    };
    let now = signing::now_unix();
    if let Err(e) = signing::verify(&key.secret, &canonical, &p.signature, now) {
        log::warn!(
            "[signing] box {} rejected on {} {}: {}",
            key.box_id,
            parts.method,
            parts.uri.path(),
            e.code()
        );
        let mut resp = refuse(e.code(), "signature rejected");
        // Hand back our clock so a skewed appliance can correct itself and retry rather than sit
        // permanently rejected. This is the difference between signing that works in a datacentre
        // and signing that works on a box in a warehouse.
        if let signing::VerifyError::Skew { server_now } = e {
            if let Ok(v) = server_now.to_string().parse() {
                resp.headers_mut().insert("x-sentinel-time", v);
            }
        }
        return resp;
    }

    // The bearer and the key must name the same box. Without this a stolen token could be paired
    // with any key the thief holds, and vice versa.
    if let Some(bearer_box) = box_id_for_bearer(&st, &parts.headers).await {
        if bearer_box != key.box_id {
            log::warn!(
                "[signing] token/key mismatch: token names {bearer_box}, key names {}",
                key.box_id
            );
            return refuse("bad_signature", "signature rejected");
        }
    }

    parts.extensions.insert(VerifiedBox {
        box_id: key.box_id,
        tenant_id: key.tenant_id,
        key_id: key.id,
    });
    mark_seen(&st, key.box_id, true).await;
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// Whether an unsigned request may proceed, given the calling box's `signing_mode`.
///
/// A token we cannot resolve is passed through rather than refused here: authenticating the bearer
/// is the handler's job (`box_tenant`), and duplicating it would mean two different 401s for the
/// same cause.
async fn unsigned_allowed(st: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let Some(hash) = bearer_hash(headers) else {
        return Ok(());
    };
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, signing_mode FROM tenant_boxes WHERE token_hash = $1")
            .bind(&hash)
            .fetch_optional(&st.pool)
            .await
            .unwrap_or(None);
    match row {
        Some((box_id, mode)) if mode == "required" => {
            log::warn!("[signing] box {box_id} is 'required' but sent an unsigned request");
            Err(refuse(
                "signature_required",
                "this box must sign its requests",
            ))
        }
        Some((box_id, _)) => {
            mark_seen(st, box_id, false).await;
            Ok(())
        }
        None => Ok(()),
    }
}

struct KeyRow {
    id: Uuid,
    box_id: Uuid,
    tenant_id: Uuid,
    secret: String,
}

/// Fetch a key that is neither revoked nor past its rotation grace.
///
/// `expires_at` is compared as text, which is only sound because both sides come from
/// `timeutil` on this host — the same rule the rest of the schema follows.
async fn live_key(st: &AppState, key_id: Uuid) -> Result<Option<KeyRow>, Response> {
    let row: Option<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, box_id, tenant_id, secret FROM box_signing_keys
         WHERE id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > $2)",
    )
    .bind(key_id)
    .bind(now_rfc3339())
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| {
        log::error!("[signing] key lookup failed: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response()
    })?;
    Ok(row.map(|(id, box_id, tenant_id, secret)| KeyRow {
        id,
        box_id,
        tenant_id,
        secret,
    }))
}

fn bearer_hash(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|t| hex::encode(Sha256::digest(t.as_bytes())))
}

async fn box_id_for_bearer(st: &AppState, headers: &HeaderMap) -> Option<Uuid> {
    let hash = bearer_hash(headers)?;
    sqlx::query_scalar("SELECT id FROM tenant_boxes WHERE token_hash = $1")
        .bind(&hash)
        .fetch_optional(&st.pool)
        .await
        .ok()
        .flatten()
}

/// Record that this box sent a signed (or unsigned) request.
///
/// Rate-limited to one write a minute per box per kind: this runs on every edge request, and the
/// value is only ever read by a human deciding whether a box is ready to enforce.
///
/// `last_unsigned_at` is the one that matters. `last_signed_at` proves only that *some* path signs;
/// a `last_unsigned_at` that has gone stale proves *every* path does — including clip-confirm and
/// disk-alert, which fire rarely enough to be missed in a refactor and would otherwise surface as
/// 401s days after enforcement.
async fn mark_seen(st: &AppState, box_id: Uuid, signed: bool) {
    let col = if signed {
        "last_signed_at"
    } else {
        "last_unsigned_at"
    };
    let sql = format!(
        "UPDATE tenant_boxes SET {col} = $2 WHERE id = $1 AND ({col} IS NULL OR {col} < $3)"
    );
    let _ = sqlx::query(&sql)
        .bind(box_id)
        .bind(now_rfc3339())
        .bind(app_core::timeutil::plus_secs_rfc3339(-60))
        .execute(&st.pool)
        .await;
}

// ---- bootstrap --------------------------------------------------------------

/// `POST /config/signing-key` — hand this box its signing key.
///
/// Authenticated by the bearer and **unsigned by construction**: it is how a box that has no key
/// gets one, so it cannot require one. That makes the bearer the root of trust for signing, which
/// is worth stating plainly — this does not defend a stolen `edge.env`. What it buys is per-request
/// integrity against anyone who never had the file, plus rotation and revocation, which did not
/// exist before.
///
/// Re-servable on purpose, unlike the "shown once" pattern used for box tokens: an appliance that
/// restarts after fetching but before persisting must be able to ask again instead of orphaning a
/// key on every boot. Nothing is protected by refusing, since the same bearer could mint a fresh
/// key anyway.
pub async fn issue_key(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let (tenant_id, box_id) = match crate::http::box_identity(&st, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };

    let existing: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, secret FROM box_signing_keys
         WHERE box_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > $2)
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(box_id)
    .bind(now_rfc3339())
    .fetch_optional(&st.pool)
    .await
    .unwrap_or(None);

    let (key_id, secret) = match existing {
        Some(pair) => pair,
        None => {
            let secret = signing::new_secret();
            let id: Uuid = match sqlx::query_scalar(
                "INSERT INTO box_signing_keys (box_id, tenant_id, secret, created_at)
                 VALUES ($1, $2, $3, $4) RETURNING id",
            )
            .bind(box_id)
            .bind(tenant_id)
            .bind(&secret)
            .bind(now_rfc3339())
            .fetch_one(&st.pool)
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    log::error!("[signing] mint failed for box {box_id}: {e}");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "mint failed").into_response();
                }
            };
            log::info!("[signing] minted key {id} for box {box_id}");
            (id, secret)
        }
    };

    Json(json!({
        "key_id": key_id,
        "secret": secret,
        "algo": "hmac-sha256",
        // Lets the edge correct its clock before its first signed request, rather than discovering
        // the skew as a 401.
        "server_time": signing::now_unix(),
    }))
    .into_response()
}

/// Revoke every live key for a box. Called when the box token is rotated at re-enrollment —
/// otherwise a key stays valid for a box whose bearer has changed hands.
pub async fn revoke_keys(pool: &sqlx::PgPool, box_id: Uuid) {
    if let Err(e) = sqlx::query(
        "UPDATE box_signing_keys SET revoked_at = $2 WHERE box_id = $1 AND revoked_at IS NULL",
    )
    .bind(box_id)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    {
        log::error!("[signing] revoke failed for box {box_id}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdrs(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    const KID: &str = "0f5a2b3c-4d5e-4f60-8a1b-2c3d4e5f6071";

    /// A request from an appliance that has not been updated yet. It must be distinguishable from
    /// a broken one, because the whole rollout depends on serving these unchanged.
    #[test]
    fn no_headers_at_all_is_unsigned_not_invalid() {
        assert!(presented(&hdrs(&[])).is_none());
        assert!(presented(&hdrs(&[("authorization", "Bearer sbx_x")])).is_none());
    }

    /// A partial set is refused rather than treated as unsigned. Otherwise dropping one header
    /// would be a way to opt out of verification.
    #[test]
    fn a_partial_signature_is_a_refusal_not_an_opt_out() {
        for partial in [
            vec![("x-sentinel-key-id", KID)],
            vec![("x-sentinel-signature", "v1=abc")],
            vec![
                ("x-sentinel-key-id", KID),
                ("x-sentinel-timestamp", "1800000000"),
                ("x-sentinel-nonce", "aa"),
            ],
        ] {
            assert_eq!(
                presented(&hdrs(&partial)).map(|r| r.is_err()),
                Some(true),
                "{partial:?} must not read as unsigned"
            );
        }
    }

    /// Values that are present but nonsense are refusals too — a non-numeric timestamp or a
    /// non-uuid key id must never reach the verifier.
    #[test]
    fn unparseable_values_are_refused() {
        let bad = hdrs(&[
            ("x-sentinel-key-id", "not-a-uuid"),
            ("x-sentinel-timestamp", "1800000000"),
            ("x-sentinel-nonce", "aa"),
            ("x-sentinel-signature", "v1=abc"),
        ]);
        assert_eq!(presented(&bad).map(|r| r.is_err()), Some(true));

        let bad_ts = hdrs(&[
            ("x-sentinel-key-id", KID),
            ("x-sentinel-timestamp", "yesterday"),
            ("x-sentinel-nonce", "aa"),
            ("x-sentinel-signature", "v1=abc"),
        ]);
        assert_eq!(presented(&bad_ts).map(|r| r.is_err()), Some(true));
    }

    #[test]
    fn a_complete_signature_parses() {
        let ok = hdrs(&[
            ("x-sentinel-key-id", KID),
            ("x-sentinel-timestamp", "1800000000"),
            ("x-sentinel-nonce", "a1b2"),
            ("x-sentinel-signature", "v1=abc"),
        ]);
        let p = presented(&ok).unwrap().unwrap();
        assert_eq!(p.key_id.to_string(), KID);
        assert_eq!(p.timestamp, 1_800_000_000);
        assert_eq!(p.nonce, "a1b2");
        assert_eq!(p.signature, "v1=abc");
    }

    /// The bearer hash must match what the box registry stores, or every signed request would fail
    /// the token/key binding check.
    #[test]
    fn bearer_hash_matches_the_registry_encoding() {
        let h = hdrs(&[("authorization", "Bearer sbx_abc")]);
        assert_eq!(
            bearer_hash(&h).unwrap(),
            hex::encode(Sha256::digest(b"sbx_abc"))
        );
        assert!(bearer_hash(&hdrs(&[])).is_none());
        assert!(bearer_hash(&hdrs(&[("authorization", "Bearer   ")])).is_none());
    }
}
