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
        log::info!("seed: provisioned Admin+Viewer roles for {provisioned} tenant(s) that had none");
    }
    // A permission added after a tenant existed would otherwise reach nobody: role grants
    // are seeded only at tenant creation, and the grant screen sits behind `roles.manage`.
    let granted = store::sync_system_roles(pool, &rbac::viewer_perms()).await?;
    if granted > 0 {
        log::info!("seed: granted {granted} new permission(s) to system roles");
    }

    // Master (SaaS administrator) — no tenant, is_master.
    let master_email = std::env::var("MASTER_EMAIL").unwrap_or_else(|_| "master@sentinel.local".into());
    let master_pw = std::env::var("MASTER_PASSWORD").unwrap_or_else(|_| "change-me-master".into());
    if store::find_user_by_email(pool, &master_email).await?.is_none() {
        if let Ok(hash) = auth::hash_password(&master_pw) {
            store::create_user(pool, None, &master_email, Some(&hash), true).await?;
            log::info!("seeded master account: {master_email}");
        }
    }

    // Demo tenant + admin (only if the slug is free).
    let demo_email = std::env::var("DEMO_ADMIN_EMAIL").unwrap_or_else(|_| "admin@demo.local".into());
    let demo_pw = std::env::var("DEMO_ADMIN_PASSWORD").unwrap_or_else(|_| "demo12345".into());
    if !store::tenant_slug_taken(pool, "demo").await?
        && store::find_user_by_email(pool, &demo_email).await?.is_none()
    {
        if let Ok(hash) = auth::hash_password(&demo_pw) {
            store::provision_tenant_with_admin(pool, "Demo", "demo", &demo_email, Some(&hash)).await?;
            log::info!("seeded Demo tenant + admin: {demo_email}");
        }
    }

    // Shared global workflow templates — the starter curriculum every tenant can clone (#136).
    crate::seed_templates::run(pool).await?;

    Ok(())
}
