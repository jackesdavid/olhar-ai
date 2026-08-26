//! Self-service password reset (card #166).
//!
//! Before this, the only reset was `POST /users/:id/password` — an **admin** setting **another**
//! user's password. A self-serve customer who forgot theirs had no way back into an account they
//! were already paying for, which is the largest silent churn source a subscription product can
//! have.
//!
//! ## The three rules that make this safe
//!
//! 1. **The request endpoint always answers 200.** Saying "no such account" turns the form into an
//!    oracle that confirms which email addresses have accounts here — for a camera product, that
//!    leaks who owns cameras. Same response, same shape, whether or not the user exists.
//! 2. **Only a hash of the token is stored.** The raw token lives in the user's inbox and in memory
//!    for the length of one request. A dump of `password_reset_tokens` is not enough to take an
//!    account.
//! 3. **Single use, short expiry.** `used_at` is stamped the moment a token is spent, so a shared
//!    browser, a forwarded email or a later inbox compromise cannot replay it.
//!
//! Changing a password also **revokes every existing session** (`set_password_and_revoke` bumps
//! `sessions_valid_from`) — the point of a reset is usually that someone else is in the account.
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{auth, store, AppState};

/// How long a link is good for. Long enough to walk to another device and read the mail, short
/// enough that a stale inbox is not a standing key to the account.
const VALID_MINUTES: u32 = 30;

/// Where the reset link points. Reuses the WebAuthn origin, which prod already sets correctly, so
/// there is one answer to "what is the app's public URL" rather than two that can disagree.
fn app_origin() -> String {
    std::env::var("APP_ORIGIN")
        .ok()
        .or_else(|| std::env::var("WEBAUTHN_RP_ORIGIN").ok())
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://localhost:5173".to_string())
}

/// The language to write to a user in — `users.locale`, English when unset (every account created
/// before the column existed). A failed lookup is English too: an email in a plausible language
/// always beats no email, and this is the locked-out path.
async fn user_lang(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> app_core::notification::templates::Lang {
    let tag: Option<String> = sqlx::query_scalar("SELECT locale FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    app_core::notification::templates::Lang::from_opt_tag(tag.as_deref())
}

fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// 32 bytes of randomness, hex — 256 bits, so guessing is not a strategy.
fn new_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

#[derive(Deserialize)]
pub struct RequestReq {
    pub email: String,
}

/// `POST /auth/password-reset/request` — "email me a link".
///
/// **Always 204**, whatever happens. The work after the lookup is best-effort by design: a mail
/// provider outage must not become an account-enumeration oracle, and the send is queued anyway so
/// it survives one.
pub async fn request(State(st): State<AppState>, Json(b): Json<RequestReq>) -> StatusCode {
    let email = b.email.trim().to_lowercase();
    if email.is_empty() {
        return StatusCode::NO_CONTENT;
    }

    let Ok(Some(user)) = store::find_user_by_email(&st.pool, &email).await else {
        // No account, or the lookup failed. Same answer either way — see rule 1.
        return StatusCode::NO_CONTENT;
    };
    // A master account is not resettable by email; `set_password_and_revoke` refuses it anyway, so
    // issuing a token would create a link that silently does nothing.
    if user.is_master {
        return StatusCode::NO_CONTENT;
    }
    // Every write below is tenant-scoped, and a user with no tenant has nothing to scope to.
    let Some(tenant_id) = user.tenant_id else {
        return StatusCode::NO_CONTENT;
    };

    let raw = new_token();
    let now = app_core::timeutil::now_rfc3339();
    let expires = (chrono::Utc::now() + chrono::Duration::minutes(VALID_MINUTES as i64))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Invalidate anything outstanding first: asking again should replace the old link, not leave
    // two live keys to the same account.
    let _ = sqlx::query("DELETE FROM password_reset_tokens WHERE user_id = $1 AND used_at IS NULL")
        .bind(user.id)
        .execute(&st.pool)
        .await;

    if let Err(e) = sqlx::query(
        "INSERT INTO password_reset_tokens (token_hash, user_id, tenant_id, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(hash_token(&raw))
    .bind(user.id)
    .bind(tenant_id)
    .bind(&now)
    .bind(&expires)
    .execute(&st.pool)
    .await
    {
        log::error!("password reset: could not store token for {email}: {e}");
        return StatusCode::NO_CONTENT;
    }

    let url = format!("{}/reset?t={raw}", app_origin());
    let lang = user_lang(&st.pool, user.id).await;
    let mail =
        app_core::notification::templates::password_reset(&email, "", &url, VALID_MINUTES, lang);
    app_core::work_queue::enqueue_email_blocking(
        &st.db,
        tenant_id,
        &email,
        &mail.subject,
        &mail.text,
        mail.html.as_deref(),
    );
    log::info!("password reset requested for {email} (link valid {VALID_MINUTES}m)");
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct ConfirmReq {
    pub token: String,
    pub password: String,
}

/// `POST /auth/password-reset/confirm` — spend the token, set the password.
///
/// Unlike [`request`], this one DOES report failure: the user is holding a link they believe in, and
/// "that link has expired, ask for another" is the only message that lets them recover. There is no
/// enumeration risk here because the token, not the email, is the secret.
pub async fn confirm(
    State(st): State<AppState>,
    Json(b): Json<ConfirmReq>,
) -> Result<StatusCode, (StatusCode, String)> {
    if b.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters".into(),
        ));
    }
    let bad_link = || {
        (
            StatusCode::BAD_REQUEST,
            "this reset link is invalid or has expired — request a new one".to_string(),
        )
    };

    let hash = hash_token(b.token.trim());
    let row: Option<(Uuid, Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT user_id, tenant_id, expires_at, used_at FROM password_reset_tokens
          WHERE token_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(&st.pool)
    .await
    .map_err(|e| {
        log::error!("password reset confirm lookup failed: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, "try again".to_string())
    })?;

    let Some((user_id, tenant_id, expires_at, used_at)) = row else {
        return Err(bad_link());
    };
    if used_at.is_some() {
        return Err(bad_link());
    }
    // RFC3339 text compares correctly against a UTC "Z" timestamp of the same shape, which is what
    // both sides generate — the project stores timestamps as text (sqlx has no chrono feature here).
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    if expires_at.as_str() <= now.as_str() {
        return Err(bad_link());
    }

    let password_hash =
        auth::hash_password(&b.password).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let now_unix = chrono::Utc::now().timestamp();
    let changed =
        store::set_password_and_revoke(&st.pool, tenant_id, user_id, &password_hash, now_unix)
            .await
            .map_err(|e| {
                log::error!("password reset: set_password failed for {user_id}: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "try again".to_string())
            })?;
    if changed == 0 {
        return Err(bad_link());
    }

    // Spend the token only after the password actually changed: if the update failed, the user must
    // still be able to use their link.
    let _ = sqlx::query("UPDATE password_reset_tokens SET used_at = $2 WHERE token_hash = $1")
        .bind(&hash)
        .bind(app_core::timeutil::now_rfc3339())
        .execute(&st.pool)
        .await;

    // Tell the account holder it happened. This mail exists for one reader — the person whose
    // account was just taken — and is worth sending even though everyone else deletes it.
    if let Ok(Some(user)) = store::find_user_by_id(&st.pool, user_id).await {
        let when = chrono::Utc::now()
            .format("%e %B %Y at %H:%M UTC")
            .to_string();
        let lang = user_lang(&st.pool, user_id).await;
        let mail = app_core::notification::templates::password_changed(
            &user.email,
            "",
            when.trim(),
            &format!("{}/login", app_origin()),
            lang,
        );
        app_core::work_queue::enqueue_email_blocking(
            &st.db,
            tenant_id,
            &user.email,
            &mail.subject,
            &mail.text,
            mail.html.as_deref(),
        );
    }

    let mut ev = app_core::events::NewEvent::new(tenant_id, "auth.password_reset", "user");
    ev.title = Some("Password reset".to_string());
    ev.message = Some("Password was reset from an emailed link".to_string());
    ev.user_id = Some(user_id);
    st.event_queue.push(ev);
    log::info!("password reset completed for user {user_id}");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored value must never be the token itself.
    #[test]
    fn the_token_is_stored_only_as_a_hash() {
        let raw = new_token();
        let h = hash_token(&raw);
        assert_ne!(h, raw);
        assert_eq!(h.len(), 64, "sha256 hex");
        assert_eq!(h, hash_token(&raw), "hashing is stable");
    }

    #[test]
    fn tokens_are_long_and_unique() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), 64, "32 bytes of randomness, hex");
        assert_ne!(a, b);
    }

    /// The expiry check is a string comparison on RFC3339 text, which only works if both sides are
    /// the same shape: UTC, seconds precision, trailing Z. Lock that in.
    #[test]
    fn rfc3339_text_ordering_matches_time_ordering() {
        let past = (chrono::Utc::now() - chrono::Duration::minutes(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let future = (chrono::Utc::now() + chrono::Duration::minutes(30))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert!(
            past.as_str() <= now.as_str(),
            "an expired token must read as expired"
        );
        assert!(
            future.as_str() > now.as_str(),
            "a live token must read as live"
        );
        assert!(
            future.ends_with('Z') && now.ends_with('Z'),
            "both sides must be UTC 'Z'"
        );
    }

    #[test]
    fn app_origin_has_no_trailing_slash_so_links_are_well_formed() {
        std::env::set_var("APP_ORIGIN", "https://app.olharai.com/");
        assert_eq!(app_origin(), "https://app.olharai.com");
        std::env::remove_var("APP_ORIGIN");
    }
}
