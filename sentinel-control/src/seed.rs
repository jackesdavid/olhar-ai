//! Idempotent seeder: the permission catalog, the Master (SaaS admin), and a Demo
//! tenant with an admin — so there's something to log in with on a fresh database.
use sqlx::PgPool;

use crate::{auth, rbac, store};

pub async fn run(pool: &PgPool) -> sqlx::Result<()> {
    // Permission catalog (the matrix columns).
    for (key, desc) in rbac::PERMISSIONS {
        store::upsert_permission(pool, key, desc).await?;
    }
    // A tenant backfilled by an old migration (the `Default` one) never went through
    // provisioning and has no roles at all — give it the standard pair first.
    let provisioned = store::ensure_tenant_roles(pool).await?;
    if provisioned > 0 {
        log::info!(
            "seed: provisioned Admin+Viewer roles for {provisioned} tenant(s) that had none"
        );
    }
    // A permission added after a tenant existed would otherwise reach nobody: role grants
    // are seeded only at tenant creation, and the grant screen sits behind `roles.manage`.
    let granted =
        store::sync_system_roles(pool, &rbac::viewer_perms(), rbac::owner_only_perms()).await?;
    if granted > 0 {
        log::info!("seed: granted {granted} new permission(s) to system roles");
    }

    // Master (SaaS administrator) — no tenant, is_master. A master login bypasses every action
    // except `live.view` and can assume any tenant, so a default password here is a public master
    // key to the whole platform. In a release build we would rather have NO master account than a
    // guessable one: an operator who forgot the env var gets a loud error and a redeploy, not a
    // silent back door. Dev keeps the convenience — same shape as `untenanted_dev_config()`, where
    // the escape hatch is a property of the binary and no stray env var can re-enable it.
    let master_email =
        std::env::var("MASTER_EMAIL").unwrap_or_else(|_| "master@sentinel.local".into());
    match seed_password(std::env::var("MASTER_PASSWORD").ok(), "change-me-master") {
        None => log::error!(
            "MASTER_PASSWORD unset in a release build — master account NOT seeded. \
             Set it and redeploy if you need one."
        ),
        Some(master_pw) => {
            if store::find_user_by_email(pool, &master_email)
                .await?
                .is_none()
            {
                if let Ok(hash) = auth::hash_password(&master_pw) {
                    store::create_user(pool, None, &master_email, Some(&hash), true).await?;
                    log::info!("seeded master account: {master_email}");
                }
            }
        }
    }

    // Demo tenant + admin (only if the slug is free). Dev convenience only: this seeded a real
    // Admin on a real tenant with a published password, on a public login endpoint that has no
    // lockout — so in a release build it is not seeded at all, with or without an env var.
    if cfg!(debug_assertions) {
        let demo_email =
            std::env::var("DEMO_ADMIN_EMAIL").unwrap_or_else(|_| "admin@demo.local".into());
        let demo_pw = std::env::var("DEMO_ADMIN_PASSWORD").unwrap_or_else(|_| "demo12345".into());
        if !store::tenant_slug_taken(pool, "demo").await?
            && store::find_user_by_email(pool, &demo_email)
                .await?
                .is_none()
        {
            if let Ok(hash) = auth::hash_password(&demo_pw) {
                store::provision_tenant_with_admin(pool, "Demo", "demo", &demo_email, Some(&hash))
                    .await?;
                log::info!("seeded Demo tenant + admin: {demo_email}");
            }
        }
    }

    // Shared global workflow templates — the starter curriculum every tenant can clone (#136).
    crate::seed_templates::run(pool).await?;

    Ok(())
}

/// The password to seed an account with, or `None` to skip seeding it entirely.
///
/// Split out from the env lookup so the decision is testable without a process environment.
/// `debug` is the caller's `cfg!(debug_assertions)`.
fn seed_password(configured: Option<String>, dev_default: &str) -> Option<String> {
    match configured {
        Some(pw) if !pw.trim().is_empty() => Some(pw),
        // A built-in default is a credential anyone can read in the source. Dev needs one to be
        // usable at all; a release build must never invent one.
        _ if cfg!(debug_assertions) => Some(dev_default.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: no configured password must never mean "use the one from the source" in a
    /// shipped binary. This is the check that would have prevented a public admin login.
    #[test]
    fn seed_password_refuses_to_invent_one_in_release() {
        let got = seed_password(None, "change-me");
        if cfg!(debug_assertions) {
            assert_eq!(got.as_deref(), Some("change-me"), "dev stays convenient");
        } else {
            assert_eq!(got, None, "release must skip seeding, not guess");
        }
    }

    /// An env var set to "" or "   " is an operator who meant to set it and didn't — treat it as
    /// unset rather than seeding an account with a blank or whitespace password.
    #[test]
    fn seed_password_treats_blank_as_unset() {
        assert_eq!(
            seed_password(Some("  ".into()), "dev"),
            seed_password(None, "dev")
        );
        assert_eq!(
            seed_password(Some(String::new()), "dev"),
            seed_password(None, "dev")
        );
    }

    /// A real value always wins, in every build profile.
    #[test]
    fn seed_password_uses_the_configured_value() {
        assert_eq!(
            seed_password(Some("s3cret".into()), "dev").as_deref(),
            Some("s3cret")
        );
    }
}
