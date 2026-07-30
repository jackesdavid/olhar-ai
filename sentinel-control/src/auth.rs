//! Auth primitives: JWT sessions, argon2 password hashing, and the `UserClaims`
//! request extractor. Authorization is **action-based** — `require(claims, action)` is
//! the only guard; nothing ever checks a role name. The `master` flag bypasses every
//! action EXCEPT `live.view` (tenant live feeds are private even from the SaaS admin).
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::{header::AUTHORIZATION, request::Parts, StatusCode};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

/// The one tenant-private action the master bypass must never grant.
pub const LIVE_VIEW: &str = "live.view";

/// The `scope` value of a media token — see [`Claims::scope`] and [`verify_media`].
pub const MEDIA_SCOPE: &str = "media";

/// How long a media token lives, in seconds.
///
/// The plan proposed 120s. That is right for a one-shot `<img src>` and wrong for this product: a
/// monitoring dashboard sits open all day, and a native `EventSource` retries the SAME URL it was
/// created with, so a 2-minute token forces a reconnect every 2 minutes for as long as the tab is
/// open. An hour is the compromise — still 24x tighter than the 24h session token it replaces.
///
/// The TTL is the *secondary* control anyway. The one that matters is that a media token is refused
/// by [`UserClaims`], so leaking one never yields API access, however long it lives.
fn media_ttl_secs() -> i64 {
    std::env::var("MEDIA_TOKEN_TTL_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(3600)
}

/// JWT body. `perms` is the user's **effective action set** (union of their roles'
/// permissions), resolved at login — so guards are pure action checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,         // user id (uuid)
    pub tid: Option<String>, // tenant id (uuid); None for the master's own token
    pub master: bool,        // SaaS admin
    #[serde(default)]
    pub act_as: bool, // master operating inside a tenant (assume)
    pub perms: Vec<String>,  // effective permissions (actions)
    pub exp: usize,          // expiry (unix seconds)
    /// Token audience (security plan §1.8). `None` = an ordinary session token. `Some("media")` =
    /// a short-lived token minted for URLs that **cannot carry a header** (`<img src>`,
    /// `EventSource`, a WebSocket), and which [`UserClaims`] refuses — so a token that leaks via
    /// browser history, `Referer`, or a proxy access log cannot be used to call the API.
    ///
    /// `serde(default)` so session tokens issued before this field existed still decode, and
    /// `skip_serializing_if` so an ordinary session token stays byte-identical in shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl Claims {
    /// Whether this token may perform `action`. Master grants everything except the
    /// tenant-private live feed.
    pub fn allows(&self, action: &str) -> bool {
        self.perms.iter().any(|p| p == action) || (self.master && action != LIVE_VIEW)
    }

    /// The tenant this token is scoped to, if any.
    pub fn tenant(&self) -> Option<Uuid> {
        self.tid.as_deref().and_then(|t| Uuid::parse_str(t).ok())
    }

    /// Whether this is a media token rather than a session (security plan §1.8).
    pub fn is_media(&self) -> bool {
        self.scope.as_deref() == Some(MEDIA_SCOPE)
    }

    /// A media token derived from this session: same identity, same permissions, short expiry, and
    /// `scope: media` so it is refused everywhere except the header-less media surfaces.
    ///
    /// Same permissions on purpose — the media handlers still call [`require`], so a token that
    /// dropped them would simply 403. The narrowing here is the *audience*, not the action set.
    pub fn to_media(&self) -> Claims {
        Claims {
            sub: self.sub.clone(),
            tid: self.tid.clone(),
            master: self.master,
            act_as: self.act_as,
            perms: self.perms.clone(),
            exp: expiry_secs(media_ttl_secs()),
            scope: Some(MEDIA_SCOPE.to_string()),
        }
    }
}

/// Action guard — the ONLY authorization primitive (no role-name checks).
pub fn require(claims: &Claims, action: &str) -> Result<(), (StatusCode, String)> {
    if claims.allows(action) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            format!("missing permission: {action}"),
        ))
    }
}

/// 24-hour expiry timestamp.
pub fn expiry_24h() -> usize {
    expiry_secs(24 * 3600)
}

/// An expiry `secs` from now, as a unix timestamp.
pub fn expiry_secs(secs: i64) -> usize {
    (chrono::Utc::now() + chrono::Duration::seconds(secs)).timestamp() as usize
}

/// Verify a token presented in a **query string** on a media surface (`<img src>`, `EventSource`)
/// — the surfaces that cannot send an `Authorization` header.
///
/// Phase 1 of the §1.8 rollout accepts **both** a media token and an ordinary session token,
/// because control, the gateway and the app cannot deploy in the same instant and a hard cutover
/// would break every tab that is already open. The session-token path is logged so phase 2
/// (refusing it) can be scheduled on evidence that nothing still relies on it, rather than on hope.
///
/// This is deliberately NOT symmetric with [`UserClaims`], which refuses media tokens outright from
/// day one. Widening what a media token can reach would be the dangerous direction; widening what a
/// media *surface* accepts is the safe one.
pub fn verify_media(secret: &str, token: &str) -> Result<Claims, String> {
    let claims = verify(secret, token)?;
    if !claims.is_media() {
        log::warn!(
            "§1.8 phase 1: session token used on a media surface (sub={}) — phase 2 will refuse it",
            claims.sub
        );
    }
    Ok(claims)
}

pub fn issue(secret: &str, claims: &Claims) -> Result<String, String> {
    jsonwebtoken::encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| e.to_string())
}

/// The JSON a successful login returns to the SPA.
#[derive(Serialize)]
pub struct SessionResp {
    pub token: String,
    pub user_id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub email: String,
    pub is_master: bool,
    pub perms: Vec<String>,
}

/// Build a signed session for a user — the shared tail of every login path (password, OAuth,
/// passkey). `Ok(None)` when the user id no longer resolves.
pub async fn issue_session(
    pool: &sqlx::PgPool,
    jwt_secret: &str,
    uid: Uuid,
) -> sqlx::Result<Option<SessionResp>> {
    let Some(user) = crate::store::find_user_by_id(pool, uid).await? else {
        return Ok(None);
    };
    let perms = if user.is_master {
        Vec::new()
    } else {
        crate::store::effective_perms(pool, user.id).await?
    };
    let claims = Claims {
        sub: user.id.to_string(),
        tid: user.tenant_id.map(|t| t.to_string()),
        master: user.is_master,
        act_as: false,
        perms: perms.clone(),
        exp: expiry_24h(),
        scope: None,
    };
    let token = issue(jwt_secret, &claims).unwrap_or_default();
    Ok(Some(SessionResp {
        token,
        user_id: user.id,
        tenant_id: user.tenant_id,
        email: user.email,
        is_master: user.is_master,
        perms,
    }))
}

pub fn verify(secret: &str, token: &str) -> Result<Claims, String> {
    jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|e| e.to_string())
}

pub fn hash_password(pw: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(pw.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Extracts + validates the `Authorization: Bearer <jwt>` into [`Claims`].
pub struct UserClaims(pub Claims);

#[axum::async_trait]
impl FromRequestParts<AppState> for UserClaims {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".into()))?;
        let claims = verify(&state.jwt_secret, token)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid or expired token".into()))?;
        // A media token is not a session (security plan §1.8). This is the whole point of the
        // scope: media tokens ride in query strings, so they end up in browser history, `Referer`
        // headers and proxy access logs — and one that leaks must not be usable to call the API.
        // Refused from day one, unlike the media surfaces themselves, which still accept session
        // tokens during the phase-1 overlap.
        if claims.is_media() {
            return Err((
                StatusCode::UNAUTHORIZED,
                "media token is not valid for this endpoint".into(),
            ));
        }
        Ok(UserClaims(claims))
    }
}

#[cfg(test)]
mod media_token_tests {
    use super::*;

    fn session() -> Claims {
        Claims {
            sub: "11111111-1111-1111-1111-111111111111".into(),
            tid: Some("22222222-2222-2222-2222-222222222222".into()),
            master: false,
            act_as: false,
            perms: vec!["persons.read".into(), "workflows.read".into()],
            exp: expiry_24h(),
            scope: None,
        }
    }

    /// A media token must keep the caller's identity and permissions — the media handlers still
    /// call `require`, so dropping them would just 403. What narrows is the audience.
    #[test]
    fn a_media_token_keeps_identity_and_perms_but_gains_the_scope() {
        let s = session();
        let m = s.to_media();
        assert_eq!(m.sub, s.sub);
        assert_eq!(m.tid, s.tid);
        assert_eq!(m.perms, s.perms);
        assert!(m.is_media());
        assert!(
            !s.is_media(),
            "a session must never look like a media token"
        );
    }

    /// The bound that makes a leaked media URL less bad than a leaked session.
    #[test]
    fn a_media_token_expires_sooner_than_the_session_it_came_from() {
        let s = session();
        assert!(
            s.to_media().exp < s.exp,
            "media token must be shorter-lived than the 24h session"
        );
    }

    /// Tokens minted before `scope` existed have no such key. `serde(default)` must decode them as
    /// sessions — if this regressed, every already-issued token would be rejected on deploy.
    #[test]
    fn a_token_without_a_scope_key_still_decodes_as_a_session() {
        let json = serde_json::to_string(&session()).unwrap();
        assert!(
            !json.contains("scope"),
            "a session token must not even carry the key: {json}"
        );
        let back: Claims = serde_json::from_str(&json).unwrap();
        assert!(!back.is_media());
    }

    /// The scope has to survive the signature round-trip, or the extractor's refusal is decorative.
    #[test]
    fn the_scope_survives_issue_and_verify() {
        let secret = "test-secret-at-least-32-chars-long-xxxx";
        let token = issue(secret, &session().to_media()).unwrap();
        assert!(verify_media(secret, &token).unwrap().is_media());
        assert!(
            verify(secret, &token).unwrap().is_media(),
            "plain verify must still see the scope — it is what UserClaims keys on"
        );
    }

    /// An unrecognised scope is not a media token, but it is not a session either as far as media
    /// surfaces are concerned — it simply fails the `is_media` check and gets logged as legacy.
    #[test]
    fn an_unknown_scope_is_not_treated_as_media() {
        let mut c = session();
        c.scope = Some("something-else".into());
        assert!(!c.is_media());
    }
}
