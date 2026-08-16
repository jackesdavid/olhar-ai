//! Published legal documents and the consent record (#167).
//!
//! **Why versions are dates, not hashes.** The artifact a regulator or a court asks for is the
//! published document, and that lives in git next to this file. A date stamp is something a human
//! can match to a page; a content hash is something only this program can. Bump the constant in the
//! same commit that changes the document text — the pair is the contract.
//!
//! **Why acceptance is append-only.** [`record`] inserts; nothing updates. A user who accepts v2
//! gets a second row, and the v1 row stays. Overwriting would destroy the only evidence of what
//! they actually agreed to at the time — which is the entire point of storing it.

use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// Version of the Terms of Service currently published at `/terms`.
/// **Bump this in the same commit that edits the document.**
pub const TERMS_VERSION: &str = "2026-08-16";
/// Version of the Privacy Policy currently published at `/privacy`.
/// **Bump this in the same commit that edits the document.**
pub const PRIVACY_VERSION: &str = "2026-08-16";

/// The documents a signup must accept. Extend this and both the API check and the recorded rows
/// follow — there is no second list to keep in sync.
pub const REQUIRED_AT_SIGNUP: [(&str, &str); 2] =
    [("terms", TERMS_VERSION), ("privacy", PRIVACY_VERSION)];

/// Record acceptance of every document required at signup.
///
/// Best-effort **per row** and deliberately non-fatal: the account already exists by the time this
/// runs, so a failed insert must not leave the user unable to sign in. It logs at `error` because a
/// missing consent row is a compliance gap someone has to fix, not a curiosity — silence here would
/// be the worst outcome of the three.
pub fn record_signup_consent(
    db: &Db,
    user_id: Uuid,
    tenant_id: Option<Uuid>,
    ip: Option<&str>,
    user_agent: Option<&str>,
) {
    for (document, version) in REQUIRED_AT_SIGNUP {
        if let Err(e) = record(db, user_id, tenant_id, document, version, ip, user_agent) {
            log::error!("consent: could not record {document} v{version} for {user_id}: {e}");
        }
    }
}

/// Insert one acceptance row. Append-only — see the module note.
pub fn record(
    db: &Db,
    user_id: Uuid,
    tenant_id: Option<Uuid>,
    document: &str,
    version: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), String> {
    let pool = db.pool();
    let (document, version) = (document.to_string(), version.to_string());
    let ip = ip.map(str::to_string);
    // Long UA strings are unbounded attacker-controlled input; keep the column sane.
    let user_agent = user_agent.map(|s| s.chars().take(400).collect::<String>());
    let now = now_rfc3339();
    db.block(async move {
        sqlx::query(
            "INSERT INTO user_consents (user_id, tenant_id, document, version, accepted_at, ip, user_agent) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(user_id)
        .bind(tenant_id)
        .bind(&document)
        .bind(&version)
        .bind(&now)
        .bind(&ip)
        .bind(&user_agent)
        .execute(&pool)
        .await
        .map(|_| ())
        .map_err(|e| format!("legal::record: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two constants and the required-at-signup list must not drift apart: a document bumped
    /// without its entry here would be accepted under a stale version, and the recorded evidence
    /// would name a document that was never shown.
    #[test]
    fn required_documents_carry_the_current_versions() {
        let terms = REQUIRED_AT_SIGNUP
            .iter()
            .find(|(d, _)| *d == "terms")
            .expect("terms required at signup");
        let privacy = REQUIRED_AT_SIGNUP
            .iter()
            .find(|(d, _)| *d == "privacy")
            .expect("privacy required at signup");
        assert_eq!(terms.1, TERMS_VERSION);
        assert_eq!(privacy.1, PRIVACY_VERSION);
    }

    /// A version has to be something a human can match to a published page.
    #[test]
    fn versions_are_iso_dates() {
        for v in [TERMS_VERSION, PRIVACY_VERSION] {
            assert_eq!(v.len(), 10, "version {v} is not YYYY-MM-DD");
            assert!(
                v.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }),
                "version {v} is not YYYY-MM-DD"
            );
        }
    }
}
