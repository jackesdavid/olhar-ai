//! Async data access for tenancy + RBAC (own pool, runs on this service's runtime).
use sqlx::PgPool;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub email: String,
    pub password_hash: Option<String>,
    pub is_master: bool,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct TenantRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub status: String,
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct RoleRow {
    pub id: Uuid,
    pub name: String,
    pub is_system: bool,
}

/// A tenant member for the user-management screen: identity + their assigned role ids
/// (so the UI can pre-check the role toggles).
#[derive(serde::Serialize)]
pub struct UserListRow {
    pub id: Uuid,
    pub email: String,
    pub is_master: bool,
    pub role_ids: Vec<Uuid>,
}

// ---- users ------------------------------------------------------------------

pub async fn find_user_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, tenant_id, email, password_hash, is_master FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, tenant_id, email, password_hash, is_master FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn create_user(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    email: &str,
    password_hash: Option<&str>,
    is_master: bool,
) -> sqlx::Result<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO users (tenant_id, email, password_hash, is_master) VALUES ($1,$2,$3,$4) RETURNING id",
    )
    .bind(tenant_id)
    .bind(email)
    .bind(password_hash)
    .bind(is_master)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// The user's effective permissions = union across their roles.
pub async fn effective_perms(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT rp.permission_key
           FROM user_roles ur
           JOIN role_permissions rp ON rp.role_id = ur.role_id
          WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(k,)| k).collect())
}

/// The tenant's members with their assigned role ids.
pub async fn list_users(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<UserListRow>> {
    let users: Vec<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT id, email, is_master FROM users WHERE tenant_id = $1 ORDER BY email",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(users.len());
    for (id, email, is_master) in users {
        let roles: Vec<(Uuid,)> = sqlx::query_as("SELECT role_id FROM user_roles WHERE user_id = $1")
            .bind(id)
            .fetch_all(pool)
            .await?;
        out.push(UserListRow { id, email, is_master, role_ids: roles.into_iter().map(|(r,)| r).collect() });
    }
    Ok(out)
}

pub async fn grant_role(pool: &PgPool, user_id: Uuid, role_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- tenants ----------------------------------------------------------------

pub async fn list_tenants(pool: &PgPool) -> sqlx::Result<Vec<TenantRow>> {
    sqlx::query_as::<_, TenantRow>("SELECT id, name, slug, status FROM tenants ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn find_tenant(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<TenantRow>> {
    sqlx::query_as::<_, TenantRow>("SELECT id, name, slug, status FROM tenants WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn tenant_slug_taken(pool: &PgPool, slug: &str) -> sqlx::Result<bool> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM tenants WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await?;
    Ok(n > 0)
}

// ---- roles + permissions ----------------------------------------------------

pub async fn list_permissions(pool: &PgPool) -> sqlx::Result<Vec<(String, String)>> {
    sqlx::query_as("SELECT key, description FROM permissions ORDER BY key")
        .fetch_all(pool)
        .await
}

pub async fn upsert_permission(pool: &PgPool, key: &str, desc: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO permissions (key, description) VALUES ($1,$2)
         ON CONFLICT (key) DO UPDATE SET description = EXCLUDED.description",
    )
    .bind(key)
    .bind(desc)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_roles(pool: &PgPool, tenant_id: Uuid) -> sqlx::Result<Vec<RoleRow>> {
    sqlx::query_as::<_, RoleRow>(
        "SELECT id, name, is_system FROM roles WHERE tenant_id = $1 ORDER BY name",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

pub async fn create_role(
    pool: &PgPool,
    tenant_id: Uuid,
    name: &str,
    is_system: bool,
) -> sqlx::Result<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO roles (tenant_id, name, is_system) VALUES ($1,$2,$3) RETURNING id",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(is_system)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn role_perm_keys(pool: &PgPool, role_id: Uuid) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT permission_key FROM role_permissions WHERE role_id = $1 ORDER BY permission_key")
            .bind(role_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(k,)| k).collect())
}

/// Replace a role's permission set (the matrix row).
pub async fn set_role_permissions(pool: &PgPool, role_id: Uuid, keys: &[String]) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM role_permissions WHERE role_id = $1").bind(role_id).execute(&mut *tx).await?;
    for k in keys {
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ($1,$2) ON CONFLICT DO NOTHING")
            .bind(role_id)
            .bind(k)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

// ---- oauth ------------------------------------------------------------------

pub async fn find_oauth_user(pool: &PgPool, provider: &str, uid: &str) -> sqlx::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM oauth_identities WHERE provider = $1 AND provider_uid = $2")
            .bind(provider)
            .bind(uid)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id,)| id))
}

pub async fn link_oauth(pool: &PgPool, provider: &str, uid: &str, user_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO oauth_identities (provider, provider_uid, user_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(provider)
        .bind(uid)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- provisioning -----------------------------------------------------------

/// Create a tenant with its seeded Admin + Viewer roles (cloned from the templates),
/// create the first user, and make them the tenant Admin. Returns (tenant_id, user_id,
/// effective_perms). `password_hash` is None for social signups.
pub async fn provision_tenant_with_admin(
    pool: &PgPool,
    tenant_name: &str,
    slug: &str,
    admin_email: &str,
    password_hash: Option<&str>,
) -> sqlx::Result<(Uuid, Uuid, Vec<String>)> {
    let (tenant_id,): (Uuid,) =
        sqlx::query_as("INSERT INTO tenants (name, slug) VALUES ($1,$2) RETURNING id")
            .bind(tenant_name)
            .bind(slug)
            .fetch_one(pool)
            .await?;

    let admin_role = create_role(pool, tenant_id, "Admin", true).await?;
    let admin_keys: Vec<String> = crate::rbac::all_perms().into_iter().map(String::from).collect();
    set_role_permissions(pool, admin_role, &admin_keys).await?;

    let viewer_role = create_role(pool, tenant_id, "Viewer", true).await?;
    let viewer_keys: Vec<String> = crate::rbac::viewer_perms().into_iter().map(String::from).collect();
    set_role_permissions(pool, viewer_role, &viewer_keys).await?;

    let user_id = create_user(pool, Some(tenant_id), admin_email, password_hash, false).await?;
    grant_role(pool, user_id, admin_role).await?;

    Ok((tenant_id, user_id, admin_keys))
}
