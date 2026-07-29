//! FIDO2 / WebAuthn passkey login — board card #147, security plan §2, stage **E0**.
//!
//! E0 = enrollment + *optional* passkey login (password still works). Register endpoints are
//! authenticated (a logged-in user adds a key); the login endpoints are public. "FIDO" here means
//! WebAuthn broadly — platform authenticators (Touch ID / Face ID / Windows Hello / Android
//! passkeys) as well as roaming keys; `authenticatorAttachment` is left unset so both flow through
//! the same table.
//!
//! Challenge state is short-lived and held **in-process** (per-pod), exactly like the OAuth CSRF
//! map in [`crate::oauth`] — control is single-replica today. The stored object is the webauthn-rs
//! [`Passkey`] (JSON in `user_credentials.passkey`); `credential_id` is lifted out for lookup.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::auth::{self, UserClaims};
use crate::{store, AppState};
use app_core::timeutil::now_rfc3339;

const CHALLENGE_TTL_SECS: u64 = 300;

/// The relying party, built once from env. **RP ID must be a registrable suffix of the app
/// origin** — get it wrong and keys silently fail to assert. Prod sets `WEBAUTHN_RP_ID=olharai.com`
/// + `WEBAUTHN_RP_ORIGIN=https://app.olharai.com`; dev falls back to localhost. `None` (unset /
/// bad config) makes every endpoint answer 501, so the app cleanly degrades to password login.
static WEBAUTHN: LazyLock<Option<Webauthn>> = LazyLock::new(|| {
    let rp_id = std::env::var("WEBAUTHN_RP_ID").unwrap_or_else(|_| "localhost".into());
    let origin =
        std::env::var("WEBAUTHN_RP_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());
    let url = Url::parse(&origin).ok()?;
    let mut builder = WebauthnBuilder::new(&rp_id, &url).ok()?.rp_name("Olhar AI");
    // When RP ID is the apex but the app is on a subdomain, list the subdomain origin too.
    if let Ok(extra) = std::env::var("WEBAUTHN_EXTRA_ORIGIN") {
        if let Ok(u) = Url::parse(&extra) {
            builder = builder.append_allowed_origin(&u);
        }
    }
    builder.build().ok()
});

fn rp() -> Result<&'static Webauthn, StatusCode> {
    WEBAUTHN.as_ref().ok_or(StatusCode::NOT_IMPLEMENTED)
}

/// Registration challenge state, keyed by user id (a user registers one key at a time).
static REG: LazyLock<Mutex<HashMap<Uuid, (PasskeyRegistration, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Authentication challenge state, keyed by an opaque handle → (state, target user, issued-at).
static AUTHN: LazyLock<Mutex<HashMap<String, (PasskeyAuthentication, Uuid, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn expired(t: &Instant) -> bool {
    t.elapsed().as_secs() > CHALLENGE_TTL_SECS
}

fn bad(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn db(e: impl std::fmt::Display) -> (StatusCode, String) {
    log::error!("webauthn db: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
}

// ── register (authenticated) ────────────────────────────────────────────────────────────────

/// `POST /auth/webauthn/register/start` — begin enrolling a new authenticator for the caller.
pub async fn register_start(
    State(st): State<AppState>,
    UserClaims(claims): UserClaims,
) -> Result<Json<CreationChallengeResponse>, (StatusCode, String)> {
    let webauthn = rp().map_err(|c| (c, "webauthn not configured".into()))?;
    let uid = Uuid::parse_str(&claims.sub).map_err(bad)?;
    let user = store::find_user_by_id(&st.pool, uid)
        .await
        .map_err(db)?
        .ok_or((StatusCode::UNAUTHORIZED, "unknown user".into()))?;

    // Exclude already-registered authenticators so the user can't double-enroll one key.
    let existing = list_passkeys(&st, uid).await.map_err(db)?;
    let exclude: Vec<CredentialID> = existing.iter().map(|(_, p)| p.cred_id().clone()).collect();

    let (ccr, state) = webauthn
        .start_passkey_registration(uid, &user.email, &user.email, Some(exclude))
        .map_err(bad)?;

    let mut reg = REG.lock().unwrap();
    reg.retain(|_, (_, t)| !expired(t));
    reg.insert(uid, (state, Instant::now()));
    Ok(Json(ccr))
}

#[derive(serde::Deserialize)]
pub struct RegisterFinish {
    credential: RegisterPublicKeyCredential,
    nickname: Option<String>,
}

/// `POST /auth/webauthn/register/finish` — verify the attestation and store the passkey.
pub async fn register_finish(
    State(st): State<AppState>,
    UserClaims(claims): UserClaims,
    Json(body): Json<RegisterFinish>,
) -> Result<StatusCode, (StatusCode, String)> {
    let webauthn = rp().map_err(|c| (c, "webauthn not configured".into()))?;
    let uid = Uuid::parse_str(&claims.sub).map_err(bad)?;

    let state = {
        let mut reg = REG.lock().unwrap();
        reg.remove(&uid)
            .filter(|(_, t)| !expired(t))
            .map(|(s, _)| s)
            .ok_or((
                StatusCode::BAD_REQUEST,
                "no registration in progress".into(),
            ))?
    };

    let passkey = webauthn
        .finish_passkey_registration(&body.credential, &state)
        .map_err(bad)?;

    let cred_id = passkey.cred_id().as_ref().to_vec();
    let json = serde_json::to_value(&passkey).map_err(bad)?;
    let nick = body.nickname.filter(|s| !s.trim().is_empty());
    sqlx::query(
        "INSERT INTO user_credentials (user_id, credential_id, passkey, nickname, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(uid)
    .bind(&cred_id)
    .bind(&json)
    .bind(nick)
    .bind(now_rfc3339())
    .execute(&st.pool)
    .await
    .map_err(db)?;

    Ok(StatusCode::CREATED)
}

// ── login (public) ──────────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct LoginStart {
    email: String,
}

#[derive(serde::Serialize)]
pub struct LoginChallenge {
    handle: String,
    options: RequestChallengeResponse,
}

/// `POST /auth/webauthn/login/start` — challenge for a passkey assertion. 404 when the address has
/// no passkey (the SPA falls back to the password form). Non-resident: `allowCredentials` carries
/// the user's keys, so `email` must be supplied.
pub async fn login_start(
    State(st): State<AppState>,
    Json(body): Json<LoginStart>,
) -> Result<Json<LoginChallenge>, (StatusCode, String)> {
    let webauthn = rp().map_err(|c| (c, "webauthn not configured".into()))?;
    let email = body.email.trim().to_lowercase();
    let user = store::find_user_by_email(&st.pool, &email)
        .await
        .map_err(db)?
        .ok_or((StatusCode::NOT_FOUND, "no passkey for this account".into()))?;
    let passkeys: Vec<Passkey> = list_passkeys(&st, user.id)
        .await
        .map_err(db)?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    if passkeys.is_empty() {
        return Err((StatusCode::NOT_FOUND, "no passkey for this account".into()));
    }

    let (rcr, state) = webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(bad)?;
    let handle = Uuid::new_v4().to_string();
    let mut a = AUTHN.lock().unwrap();
    a.retain(|_, (_, _, t)| !expired(t));
    a.insert(handle.clone(), (state, user.id, Instant::now()));
    Ok(Json(LoginChallenge {
        handle,
        options: rcr,
    }))
}

#[derive(serde::Deserialize)]
pub struct LoginFinish {
    handle: String,
    credential: PublicKeyCredential,
}

/// `POST /auth/webauthn/login/finish` — verify the assertion, bump the signature counter, and mint
/// a session identical to the password / OAuth path.
pub async fn login_finish(
    State(st): State<AppState>,
    Json(body): Json<LoginFinish>,
) -> Result<Json<auth::SessionResp>, (StatusCode, String)> {
    let webauthn = rp().map_err(|c| (c, "webauthn not configured".into()))?;

    let (state, uid) = {
        let mut a = AUTHN.lock().unwrap();
        a.remove(&body.handle)
            .filter(|(_, _, t)| !expired(t))
            .map(|(s, u, _)| (s, u))
            .ok_or((StatusCode::BAD_REQUEST, "no login in progress".into()))?
    };

    let result = webauthn
        .finish_passkey_authentication(&body.credential, &state)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "assertion failed".into()))?;

    // Bump the stored signature counter on the credential that just asserted (clone-detection).
    if result.needs_update() {
        for (row_id, mut passkey) in list_passkeys(&st, uid).await.map_err(db)? {
            if passkey.cred_id() == result.cred_id() {
                passkey.update_credential(&result);
                let json = serde_json::to_value(&passkey).map_err(db)?;
                sqlx::query(
                    "UPDATE user_credentials SET passkey = $1, last_used_at = $2 WHERE id = $3",
                )
                .bind(&json)
                .bind(now_rfc3339())
                .bind(row_id)
                .execute(&st.pool)
                .await
                .map_err(db)?;
                break;
            }
        }
    } else {
        sqlx::query("UPDATE user_credentials SET last_used_at = $1 WHERE user_id = $2 AND credential_id = $3")
            .bind(now_rfc3339())
            .bind(uid)
            .bind(result.cred_id().as_ref())
            .execute(&st.pool)
            .await
            .map_err(db)?;
    }

    let resp = auth::issue_session(&st.pool, &st.jwt_secret, uid)
        .await
        .map_err(db)?
        .ok_or((StatusCode::UNAUTHORIZED, "unknown user".into()))?;
    Ok(Json(resp))
}

// ── manage (authenticated) — the "Security keys" section in settings ──────────────────────────

#[derive(serde::Serialize)]
pub struct CredMeta {
    id: Uuid,
    nickname: Option<String>,
    created_at: String,
    last_used_at: Option<String>,
}

/// `GET /auth/webauthn/credentials` — list the caller's registered keys.
pub async fn list_credentials(
    State(st): State<AppState>,
    UserClaims(claims): UserClaims,
) -> Result<Json<Vec<CredMeta>>, (StatusCode, String)> {
    let uid = Uuid::parse_str(&claims.sub).map_err(bad)?;
    let rows: Vec<(Uuid, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT id, nickname, created_at, last_used_at FROM user_credentials
         WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(uid)
    .fetch_all(&st.pool)
    .await
    .map_err(db)?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, nickname, created_at, last_used_at)| CredMeta {
                id,
                nickname,
                created_at,
                last_used_at,
            })
            .collect(),
    ))
}

/// `DELETE /auth/webauthn/credentials/:id` — remove one of the caller's keys.
pub async fn delete_credential(
    State(st): State<AppState>,
    UserClaims(claims): UserClaims,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let uid = Uuid::parse_str(&claims.sub).map_err(bad)?;
    let n = sqlx::query("DELETE FROM user_credentials WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(uid)
        .execute(&st.pool)
        .await
        .map_err(db)?
        .rows_affected();
    if n == 0 {
        return Err((StatusCode::NOT_FOUND, "no such credential".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── helpers ───────────────────────────────────────────────────────────────────────────────────

/// The caller's stored passkeys as `(row id, Passkey)`, deserialized from the `passkey` jsonb.
async fn list_passkeys(st: &AppState, uid: Uuid) -> sqlx::Result<Vec<(Uuid, Passkey)>> {
    let rows: Vec<(Uuid, serde_json::Value)> =
        sqlx::query_as("SELECT id, passkey FROM user_credentials WHERE user_id = $1")
            .bind(uid)
            .fetch_all(&st.pool)
            .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, v)| serde_json::from_value::<Passkey>(v).ok().map(|p| (id, p)))
        .collect())
}
