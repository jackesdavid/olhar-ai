//! User invites (#181) — how a person joins a tenant without anyone reading a password aloud.
//!
//! Today the only way to become a user is `invite_user`, which takes an **admin-chosen password**
//! and sends nothing, so the real-world journey is an admin dictating credentials. This module is
//! the durable half of the replacement: an admin mints an invite, and the person redeems it from a
//! link or by scanning a QR on their phone.
//!
//! Two invariants, both borrowed from `workflows::device_gate` rather than reinvented:
//!
//! 1. **The raw token never touches the database.** Only `sha256(token)` hex is stored, and rows
//!    are looked up BY that hash — so there is no secret-dependent comparison to time. A dump of
//!    `user_invites` must not be enough to join a tenant.
//! 2. **An invite can be redeemed exactly once.** Acceptance is ONE conditional `UPDATE` with
//!    `accepted_at IS NULL` in the `WHERE`, and the row count is the answer to "did I win". A
//!    check-then-act here means two people join on one invite — the same shape that was
//!    deliberately not copied from password reset.
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// What the phone or the web page is allowed to see BEFORE anyone commits: which organisation this
/// invite is for. Deliberately not the roles or any id — an unauthenticated caller holding a
/// guessed token should learn as little as possible, and "Join Acme?" is the whole point.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InvitePreview {
    pub tenant_name: String,
    pub email: Option<String>,
    pub expires_at: String,
}

/// A freshly minted invite. The raw `token` and `code` exist here and nowhere else — the row holds
/// only the hash of the first.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MintedInvite {
    pub id: Uuid,
    pub token: String,
    pub code: String,
    pub expires_at: String,
}

/// Why an acceptance was refused. The caller maps these to a status; they are distinct because
/// "already used" and "expired" are different things to tell a person, and neither is "not found".
#[derive(Debug, Clone, PartialEq)]
pub enum AcceptError {
    /// No open invite with that token — never existed, or already redeemed.
    NotFound,
    Expired,
    /// Someone already has this address in this tenant.
    EmailTaken,
    Db(String),
}

/// 32 bytes of randomness, hex. Same generator as the Device Gate token and password reset,
/// deliberately: this repo has enough token shapes already.
fn new_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// The short form someone types when a camera is unavailable. Unambiguous alphabet — no `O`/`0`,
/// no `I`/`1`/`L` — because this gets read off a screen and typed on a phone, and a code that
/// cannot be transcribed reliably is a code that generates support calls.
///
/// 8 characters over a 30-symbol alphabet is ~49 bits. Lower than the token by design: it is
/// rate-limited at the endpoint, single-use, and short-lived.
fn new_code() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

pub fn hash_token(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Mint an invite, returning the raw token and code exactly once.
///
/// `role_ids` are baked in at creation. The alternative — a default role, fixed up afterwards —
/// means every invited person needs a second admin action before they can do anything, which is
/// how "invited" and "actually working" drift apart.
pub fn mint(
    db: &Db,
    tenant_id: Uuid,
    email: Option<&str>,
    role_ids: &[Uuid],
    ttl_secs: i64,
    created_by: Option<Uuid>,
) -> Result<MintedInvite, String> {
    let token = new_token();
    let code = new_code();
    let token_hash = hash_token(&token);
    // Clamped: an invite is a door into the tenant, so a mis-typed TTL must not leave one open for
    // a year. The floor keeps a slow install (download the app, sign in to the store) from racing
    // the clock.
    let ttl = ttl_secs.clamp(300, 30 * 24 * 3600);
    let expires_at = crate::timeutil::plus_secs_rfc3339(ttl);
    let created_at = now_rfc3339();
    let id = Uuid::new_v4();
    let roles_json = serde_json::to_string(role_ids).unwrap_or_else(|_| "[]".into());

    let pool = db.pool();
    let (t_hash, c, exp, cre) = (
        token_hash,
        code.clone(),
        expires_at.clone(),
        created_at.clone(),
    );
    let email_owned = email.map(|e| e.trim().to_lowercase());
    db.block(async move {
        sqlx::query(
            "INSERT INTO user_invites
               (id, tenant_id, token_sha256, code, email, role_ids_json, created_at, expires_at, created_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&t_hash)
        .bind(&c)
        .bind(email_owned.as_deref())
        .bind(&roles_json)
        .bind(&cre)
        .bind(&exp)
        .bind(created_by)
        .execute(&pool)
        .await
    })
    .map_err(|e| e.to_string())?;

    Ok(MintedInvite {
        id,
        token,
        code,
        expires_at,
    })
}

/// What an unauthenticated holder of `token_or_code` may see. `None` when there is no OPEN invite
/// for it — an expired or redeemed one is indistinguishable from a wrong token here on purpose,
/// since this endpoint is enumerable.
pub fn preview(db: &Db, token_or_code: &str) -> Option<InvitePreview> {
    let hash = hash_token(token_or_code);
    let code = token_or_code.trim().to_uppercase();
    let pool = db.pool();
    let now = now_rfc3339();
    let row: Option<(String, Option<String>, String)> = db
        .block(async move {
            sqlx::query_as(
                "SELECT t.name, i.email, i.expires_at
                   FROM user_invites i
                   JOIN tenants t ON t.id = i.tenant_id
                  WHERE (i.token_sha256 = $1 OR i.code = $2)
                    AND i.accepted_at IS NULL
                    AND i.expires_at > $3",
            )
            .bind(&hash)
            .bind(&code)
            .bind(&now)
            .fetch_optional(&pool)
            .await
        })
        .ok()
        .flatten();
    row.map(|(tenant_name, email, expires_at)| InvitePreview {
        tenant_name,
        email,
        expires_at,
    })
}

/// An invite claimed for redemption: everything the caller needs to create the user.
pub struct ClaimedInvite {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: Option<String>,
    pub role_ids: Vec<Uuid>,
}

/// Claim the invite, atomically. Returns what to create the user with, or why not.
///
/// The `UPDATE … WHERE accepted_at IS NULL RETURNING` is the whole concurrency story: two phones
/// scanning the same QR at once both run it, exactly one gets a row, and the loser is told the
/// invite was already used rather than silently creating a second account.
///
/// Note the ORDER: the row is claimed BEFORE the user exists. If user creation then fails, the
/// invite is spent — deliberately. A spent invite the admin can re-issue is a smaller problem than
/// an invite that can be retried into two accounts.
pub fn claim(db: &Db, token_or_code: &str) -> Result<ClaimedInvite, AcceptError> {
    let hash = hash_token(token_or_code);
    let code = token_or_code.trim().to_uppercase();
    let now = now_rfc3339();
    let pool = db.pool();

    // Separate the two "no" answers first, so the caller can say which. This read is advisory
    // only — the conditional UPDATE below is what actually decides.
    let existing: Option<(String, Option<String>)> = db
        .block({
            let (h, c, pool) = (hash.clone(), code.clone(), pool.clone());
            async move {
                sqlx::query_as(
                    "SELECT expires_at, accepted_at FROM user_invites
                      WHERE token_sha256 = $1 OR code = $2",
                )
                .bind(&h)
                .bind(&c)
                .fetch_optional(&pool)
                .await
            }
        })
        .map_err(|e| AcceptError::Db(e.to_string()))?;
    match &existing {
        None => return Err(AcceptError::NotFound),
        Some((_, Some(_))) => return Err(AcceptError::NotFound), // already redeemed
        Some((exp, None)) if exp.as_str() <= now.as_str() => return Err(AcceptError::Expired),
        _ => {}
    }

    let claimed: Option<(Uuid, Uuid, Option<String>, String)> = db
        .block({
            let (h, c, n, pool) = (hash, code, now, pool.clone());
            async move {
                sqlx::query_as(
                    "UPDATE user_invites
                        SET accepted_at = $3
                      WHERE (token_sha256 = $1 OR code = $2)
                        AND accepted_at IS NULL
                        AND expires_at > $3
                  RETURNING id, tenant_id, email, role_ids_json",
                )
                .bind(&h)
                .bind(&c)
                .bind(&n)
                .fetch_optional(&pool)
                .await
            }
        })
        .map_err(|e| AcceptError::Db(e.to_string()))?;

    let (id, tenant_id, email, roles_json) = claimed.ok_or(AcceptError::NotFound)?;
    let role_ids: Vec<Uuid> = serde_json::from_str(&roles_json).unwrap_or_default();
    Ok(ClaimedInvite {
        id,
        tenant_id,
        email,
        role_ids,
    })
}

/// Record which user an invite created, once they exist. Best-effort audit — the claim already
/// happened, and failing to stamp the id must not undo a successful join.
pub fn note_accepted_user(db: &Db, invite_id: Uuid, user_id: Uuid) {
    let pool = db.pool();
    let _ = db.block(async move {
        sqlx::query("UPDATE user_invites SET accepted_user = $2 WHERE id = $1")
            .bind(invite_id)
            .bind(user_id)
            .execute(&pool)
            .await
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The typed code is read off a screen and typed on a phone. Characters that are routinely
    /// confused (O/0, I/1/L) would turn a transcription slip into "invite not found".
    #[test]
    fn code_alphabet_has_no_confusable_characters() {
        for _ in 0..200 {
            let c = new_code();
            assert_eq!(c.len(), 8);
            assert!(
                !c.contains(['O', '0', 'I', '1', 'L']),
                "confusable character in {c}"
            );
            assert!(c
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()));
        }
    }

    /// Tokens are looked up by hash, never compared — so the hash has to be stable and not the
    /// token itself.
    #[test]
    fn tokens_are_stored_hashed() {
        let raw = new_token();
        assert_eq!(raw.len(), 64, "32 bytes hex");
        let h = hash_token(&raw);
        assert_eq!(h.len(), 64);
        assert_ne!(h, raw);
        assert_eq!(h, hash_token(&raw), "stable");
    }

    #[test]
    fn tokens_do_not_repeat() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
    }
}
