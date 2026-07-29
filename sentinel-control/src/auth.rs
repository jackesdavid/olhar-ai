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
    (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize
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
        Ok(UserClaims(claims))
    }
}
