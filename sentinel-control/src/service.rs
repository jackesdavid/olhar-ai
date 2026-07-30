//! Service-to-service auth, and the **central token introspection** endpoint.
//!
//! Control is the identity authority: it owns users, roles, permissions, the lockout, passkeys and
//! OAuth, and it is the only component that holds `JWT_SECRET`. Other services stop decoding tokens
//! themselves and ask here instead.
//!
//! Why that is better than sharing a `Claims` type: the gateway kept a hand-copied subset of
//! control's claims, so when `scope` was added for the media-token work the gateway silently did not
//! know about it — it accepted a media token for live video while control refused the same token
//! everywhere else. A shared crate would fix that instance; a single decoder makes the whole class
//! impossible, and it lets the gateway drop `jsonwebtoken` **and** `JWT_SECRET` entirely (the
//! gateway is deliberately a lean standalone crate with no `app-core` dependency, so pulling a
//! shared type in was never free).
//!
//! It also buys the thing a self-validating JWT cannot do: **revocation**. Until now the only way to
//! invalidate a session was rotating `JWT_SECRET` — a platform-wide logout. See
//! [`introspect`] and migration 0080.

use axum::extract::{FromRequestParts, State};
use axum::http::{header::AUTHORIZATION, request::Parts, StatusCode};
use axum::response::Json;
use axum::routing::post;
use axum::Router;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::auth;
use crate::AppState;

/// The dev-only fallback service key. Same shape as `DEV_JWT_SECRET`: usable in a debug build,
/// unreachable in a release one.
const DEV_SERVICE_KEY: &str = "dev-insecure-service-key-change-me-0000000000";

/// Resolve `INTERNAL_SERVICE_KEY`, failing closed in a release build.
///
/// Mirrors `resolve_jwt_secret` deliberately — this is the second shared secret between two
/// services, and it has the same failure mode: if the two sides disagree, nothing errors loudly,
/// the calls just stop working. A release build refusing to start is far louder than a gateway that
/// quietly cannot resolve any box.
pub fn resolve_service_key(configured: Option<String>) -> Result<String, &'static str> {
    match configured {
        Some(s) if s.trim().len() >= 32 => Ok(s),
        Some(_) if cfg!(debug_assertions) => Ok(DEV_SERVICE_KEY.to_string()),
        Some(_) => Err("INTERNAL_SERVICE_KEY is set but shorter than 32 characters"),
        None if cfg!(debug_assertions) => {
            log::warn!(
                "INTERNAL_SERVICE_KEY unset — using the insecure dev key (debug build only)"
            );
            Ok(DEV_SERVICE_KEY.to_string())
        }
        None => Err("INTERNAL_SERVICE_KEY is unset"),
    }
}

/// Constant-time comparison of two service keys.
///
/// A MAC over both sides rather than `==`. Not because a timing oracle across the internet is
/// plausible, but because a shared-secret comparison is where the habit is worth keeping — the same
/// reasoning the Asaas webhook check uses.
fn keys_match(presented: &str, expected: &str) -> bool {
    let mut a = <Hmac<Sha256>>::new_from_slice(b"sentinel-service-key").expect("hmac key");
    a.update(presented.as_bytes());
    let mut b = <Hmac<Sha256>>::new_from_slice(b"sentinel-service-key").expect("hmac key");
    b.update(expected.as_bytes());
    a.verify_slice(&b.finalize().into_bytes()).is_ok()
}

/// Proof that the caller is another Sentinel service, not a user.
///
/// Deliberately NOT a fallback to `UserClaims`: an endpoint guarded by this must be unreachable by
/// an end user's token, or it is not a service boundary at all. `/internal/*` is merged into the
/// PUBLIC router, so "only the gateway calls this" has never been an access control on its own.
pub struct ServiceCaller;

#[axum::async_trait]
impl FromRequestParts<AppState> for ServiceCaller {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let presented = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or((
                StatusCode::UNAUTHORIZED,
                "service credential required".to_string(),
            ))?;
        if keys_match(presented, &state.service_key) {
            Ok(ServiceCaller)
        } else {
            // No detail: a caller learning "right shape, wrong value" learns more than it needs.
            Err((
                StatusCode::UNAUTHORIZED,
                "service credential rejected".to_string(),
            ))
        }
    }
}

// ---- introspection ----------------------------------------------------------

#[derive(Deserialize)]
pub struct IntrospectReq {
    pub token: String,
}

/// The claims a calling service needs, plus whether the token is usable at all.
///
/// Shaped after RFC 7662 (`active` first, everything else absent when inactive) so a caller cannot
/// accidentally read identity fields off a rejected token — the common failure in hand-rolled
/// introspection is trusting a body without checking the flag.
#[derive(Serialize, Default)]
pub struct IntrospectResp {
    pub active: bool,
    /// Why it was refused. Present only when `active` is false; for diagnosis, never for control flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub act_as: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perms: Option<Vec<String>>,
    /// `"media"` for a media token, absent for an ordinary session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<usize>,
}

impl IntrospectResp {
    fn inactive(reason: &'static str) -> Self {
        IntrospectResp {
            active: false,
            reason: Some(reason),
            ..Default::default()
        }
    }
}

/// Whether `iat` falls before a revocation cutoff.
///
/// A token with **no** `iat` (minted before the field existed) counts as revoked once a cutoff is
/// set. That is the direction that matters: "log this user out everywhere" must not be defeatable by
/// presenting an older token. It costs nothing on deploy because every user starts with no cutoff.
pub fn revoked(iat: Option<usize>, valid_from: Option<i64>) -> bool {
    match valid_from {
        None => false,
        Some(cutoff) => match iat {
            None => true,
            Some(issued) => (issued as i64) < cutoff,
        },
    }
}

/// `POST /internal/introspect` — the central validation call.
///
/// Service-authenticated, because it takes an arbitrary token and reports what it grants: reachable
/// by anyone, it would be an oracle for testing stolen tokens.
///
/// Two checks a local decoder cannot do, which is the entire reason this exists:
/// signature/expiry **and** revocation.
pub async fn introspect(
    _svc: ServiceCaller,
    State(st): State<AppState>,
    Json(body): Json<IntrospectReq>,
) -> Json<IntrospectResp> {
    let Ok(claims) = auth::verify(&st.jwt_secret, &body.token) else {
        // Covers a bad signature and an expired token alike; the caller does not need to know which,
        // and telling it distinguishes "wrong key" from "too old" for free.
        return Json(IntrospectResp::inactive("invalid or expired"));
    };

    let valid_from = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(uid) => match crate::store::sessions_valid_from(&st.pool, uid).await {
            Ok(v) => v,
            Err(e) => {
                // Fail CLOSED. A database blip must not silently un-revoke a session that was
                // deliberately killed — this is the one place where availability loses to
                // correctness, and it is a short outage against a permanent bypass.
                log::error!("introspect: revocation lookup failed for {uid}: {e}");
                return Json(IntrospectResp::inactive("revocation state unavailable"));
            }
        },
        Err(_) => return Json(IntrospectResp::inactive("malformed subject")),
    };

    if revoked(claims.iat, valid_from) {
        return Json(IntrospectResp::inactive("session revoked"));
    }

    Json(IntrospectResp {
        active: true,
        reason: None,
        sub: Some(claims.sub),
        tid: claims.tid,
        master: Some(claims.master),
        act_as: Some(claims.act_as),
        perms: Some(claims.perms),
        scope: claims.scope,
        exp: Some(claims.exp),
    })
}

pub fn router() -> Router<AppState> {
    Router::new().route("/internal/introspect", post(introspect))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_build_refuses_to_invent_a_service_key() {
        // Mirrors the JWT_SECRET tests: the dev fallback must be a property of the BINARY, so no
        // missing env var on the host can talk a release build into a known key.
        let r = resolve_service_key(None);
        if cfg!(debug_assertions) {
            assert!(r.is_ok(), "debug builds get the dev key");
        } else {
            assert!(r.is_err(), "release must refuse to start");
        }
    }

    #[test]
    fn a_short_key_is_rejected_in_release() {
        let r = resolve_service_key(Some("too-short".into()));
        if cfg!(debug_assertions) {
            assert!(r.is_ok());
        } else {
            assert!(r.is_err());
        }
    }

    #[test]
    fn a_real_key_is_used_in_every_build() {
        let key = "a".repeat(48);
        assert_eq!(resolve_service_key(Some(key.clone())).unwrap(), key);
    }

    #[test]
    fn keys_match_only_themselves() {
        let k = "x".repeat(48);
        assert!(keys_match(&k, &k));
        assert!(!keys_match(&k, &"y".repeat(48)));
        // Length must not be a shortcut to acceptance.
        assert!(!keys_match("short", &k));
        assert!(!keys_match("", &k));
    }

    /// No cutoff = nothing revoked. This is the state every user is in on deploy, so getting it
    /// wrong would log out the entire platform.
    #[test]
    fn without_a_cutoff_nothing_is_revoked() {
        assert!(!revoked(Some(1_000), None));
        assert!(!revoked(None, None));
    }

    #[test]
    fn a_token_issued_before_the_cutoff_is_revoked() {
        assert!(revoked(Some(999), Some(1_000)));
        assert!(
            !revoked(Some(1_000), Some(1_000)),
            "the cutoff itself is not before itself"
        );
        assert!(!revoked(Some(1_001), Some(1_000)));
    }

    /// A pre-`iat` token must not be able to outlive a revocation just by predating the field.
    #[test]
    fn a_token_with_no_iat_is_revoked_once_a_cutoff_exists() {
        assert!(revoked(None, Some(1_000)));
    }
}
