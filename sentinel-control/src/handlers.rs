//! HTTP handlers: auth (signup/login/me), RBAC admin (roles, matrix, users), and the
//! master console (tenants + navigate-as-tenant). Every guard is action-based via
//! `auth::require`; the only role-ish gate is the `master` flag.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post, put};
use axum::Router;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{self, Claims, UserClaims};
use crate::store;
use crate::AppState;

type ApiErr = (StatusCode, String);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/me", get(me))
        .route("/auth/oauth/google/start", get(crate::oauth::google_start))
        .route("/auth/oauth/google/callback", get(crate::oauth::google_callback))
        .route("/permissions", get(list_permissions))
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/:id/permissions", get(get_role_perms).put(set_role_perms))
        .route("/users", get(list_users).post(invite_user))
        .route("/users/:id/roles", put(set_user_roles))
        .route("/admin/tenants", get(list_tenants).post(create_tenant))
        .route("/admin/tenants/:id/assume", post(assume_tenant))
}

#[derive(Serialize)]
struct AuthResp {
    token: String,
    user_id: Uuid,
    tenant_id: Option<Uuid>,
    is_master: bool,
    perms: Vec<String>,
}

fn err(code: StatusCode, msg: impl Into<String>) -> ApiErr {
    (code, msg.into())
}
fn db_err(e: sqlx::Error) -> ApiErr {
    log::error!("db error: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
}

/// A DNS-safe slug from a tenant name.
fn slugify(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "tenant".into() } else { s }
}

/// Tenant context from the token (set by login/assume). Errors if absent (e.g. the
/// master's own un-assumed token has no tenant).
fn tenant_ctx(c: &Claims) -> Result<Uuid, ApiErr> {
    c.tenant().ok_or(err(StatusCode::BAD_REQUEST, "no tenant context — assume a tenant first"))
}

fn require_master(c: &Claims) -> Result<(), ApiErr> {
    if c.master { Ok(()) } else { Err(err(StatusCode::FORBIDDEN, "master only")) }
}

// ---- auth -------------------------------------------------------------------

#[derive(Deserialize)]
struct SignupReq {
    tenant_name: String,
    email: String,
    password: String,
}

/// Public signup: creates a tenant and makes the caller its Admin.
async fn signup(State(st): State<AppState>, Json(b): Json<SignupReq>) -> Result<Json<AuthResp>, ApiErr> {
    let email = b.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(err(StatusCode::BAD_REQUEST, "valid email required"));
    }
    if b.password.len() < 8 {
        return Err(err(StatusCode::BAD_REQUEST, "password must be at least 8 characters"));
    }
    if store::find_user_by_email(&st.pool, &email).await.map_err(db_err)?.is_some() {
        return Err(err(StatusCode::CONFLICT, "email already registered"));
    }
    // Unique slug (append a short suffix on collision).
    let mut slug = slugify(&b.tenant_name);
    if store::tenant_slug_taken(&st.pool, &slug).await.map_err(db_err)? {
        slug = format!("{slug}-{}", &Uuid::new_v4().to_string()[..6]);
    }
    let hash = auth::hash_password(&b.password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (tenant_id, user_id, perms) =
        store::provision_tenant_with_admin(&st.pool, b.tenant_name.trim(), &slug, &email, Some(&hash))
            .await
            .map_err(db_err)?;

    // Self-service signup starts the 30-day Beta trial (2 cameras). Best-effort: a failure
    // here must not fail the signup — the master can grant a trial later.
    {
        let db = st.db.clone();
        if let Ok(Err(e)) =
            tokio::task::spawn_blocking(move || app_core::billing::subscriptions::start_trial(&db, tenant_id)).await
        {
            log::warn!("signup: could not start trial for {tenant_id}: {e}");
        }
    }

    let claims = Claims {
        sub: user_id.to_string(),
        tid: Some(tenant_id.to_string()),
        master: false,
        act_as: false,
        perms: perms.clone(),
        exp: auth::expiry_24h(),
    };
    let token = auth::issue(&st.jwt_secret, &claims).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(AuthResp { token, user_id, tenant_id: Some(tenant_id), is_master: false, perms }))
}

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

async fn login(State(st): State<AppState>, Json(b): Json<LoginReq>) -> Result<Json<AuthResp>, ApiErr> {
    let email = b.email.trim().to_lowercase();
    let user = store::find_user_by_email(&st.pool, &email)
        .await
        .map_err(db_err)?
        .ok_or(err(StatusCode::UNAUTHORIZED, "invalid credentials"))?;
    let hash = user.password_hash.as_deref().ok_or(err(StatusCode::UNAUTHORIZED, "use social login for this account"))?;
    if !auth::verify_password(&b.password, hash) {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid credentials"));
    }
    let perms = if user.is_master { Vec::new() } else { store::effective_perms(&st.pool, user.id).await.map_err(db_err)? };
    let claims = Claims {
        sub: user.id.to_string(),
        tid: user.tenant_id.map(|t| t.to_string()),
        master: user.is_master,
        act_as: false,
        perms: perms.clone(),
        exp: auth::expiry_24h(),
    };
    let token = auth::issue(&st.jwt_secret, &claims).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // First producer on the agnostic event stream (card #66): record the sign-in. A master token
    // carries no tenant, so only tenant users land on a tenant's feed.
    if let Some(tid) = user.tenant_id {
        let mut ev = app_core::events::NewEvent::new(tid, "auth.login", "user");
        ev.title = Some("Sign-in".to_string());
        ev.message = Some(format!("{} signed in", email));
        ev.user_id = Some(user.id);
        st.event_queue.push(ev);
    }
    Ok(Json(AuthResp { token, user_id: user.id, tenant_id: user.tenant_id, is_master: user.is_master, perms }))
}

#[derive(Serialize)]
struct MeResp {
    user_id: String,
    tenant_id: Option<String>,
    is_master: bool,
    act_as: bool,
    perms: Vec<String>,
}

async fn me(UserClaims(c): UserClaims) -> Json<MeResp> {
    Json(MeResp {
        user_id: c.sub,
        tenant_id: c.tid,
        is_master: c.master,
        act_as: c.act_as,
        perms: c.perms,
    })
}

// ---- RBAC -------------------------------------------------------------------

async fn list_permissions(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let perms = store::list_permissions(&st.pool).await.map_err(db_err)?;
    Ok(Json(perms.into_iter().map(|(key, description)| serde_json::json!({ "key": key, "description": description })).collect()))
}

async fn list_roles(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<store::RoleRow>>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    Ok(Json(store::list_roles(&st.pool, tid).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct CreateRoleReq {
    name: String,
}

async fn create_role(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<CreateRoleReq>) -> Result<Json<Uuid>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "role name required"));
    }
    Ok(Json(store::create_role(&st.pool, tid, name, false).await.map_err(db_err)?))
}

async fn get_role_perms(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Vec<String>>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    // Answer the same way as a write: a role from another tenant simply doesn't exist here.
    if !store::role_in_tenant(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "role not found"));
    }
    Ok(Json(store::role_perm_keys(&st.pool, tid, id).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct SetPermsReq {
    keys: Vec<String>,
}

async fn set_role_perms(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<SetPermsReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    // The role id comes from the client. Refuse a role from another tenant outright rather
    // than letting the scoped writes silently no-op.
    if !store::role_in_tenant(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "role not found"));
    }
    // Unknown keys would silently vanish (the FK drops them); reject so the UI can't drift
    // from the catalog.
    for k in &b.keys {
        if !crate::rbac::all_perms().contains(&k.as_str()) {
            return Err(err(StatusCode::BAD_REQUEST, format!("unknown permission '{k}'")));
        }
    }
    store::set_role_permissions(&st.pool, tid, id, &b.keys).await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<store::UserListRow>>, ApiErr> {
    auth::require(&c, "users.manage")?;
    let tid = tenant_ctx(&c)?;
    Ok(Json(store::list_users(&st.pool, tid).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct InviteReq {
    email: String,
    password: String,
    role_ids: Vec<Uuid>,
}

async fn invite_user(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<InviteReq>) -> Result<Json<Uuid>, ApiErr> {
    auth::require(&c, "users.manage")?;
    let tid = tenant_ctx(&c)?;
    let email = b.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(err(StatusCode::BAD_REQUEST, "valid email required"));
    }
    if b.password.len() < 8 {
        return Err(err(StatusCode::BAD_REQUEST, "password must be at least 8 characters"));
    }
    let hash = auth::hash_password(&b.password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let uid = store::create_user(&st.pool, Some(tid), &email, Some(&hash), false).await.map_err(db_err)?;
    for r in b.role_ids {
        store::grant_role(&st.pool, uid, r).await.map_err(db_err)?;
    }
    Ok(Json(uid))
}

#[derive(Deserialize)]
struct SetUserRolesReq {
    role_ids: Vec<Uuid>,
}

async fn set_user_roles(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<SetUserRolesReq>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "users.manage")?;
    // replace-all: clear then grant
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1").bind(id).execute(&st.pool).await.map_err(db_err)?;
    for r in b.role_ids {
        store::grant_role(&st.pool, id, r).await.map_err(db_err)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- master console ---------------------------------------------------------

async fn list_tenants(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<store::TenantRow>>, ApiErr> {
    require_master(&c)?;
    Ok(Json(store::list_tenants(&st.pool).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct CreateTenantReq {
    tenant_name: String,
    admin_email: String,
    admin_password: String,
}

async fn create_tenant(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<CreateTenantReq>) -> Result<Json<Uuid>, ApiErr> {
    require_master(&c)?;
    let email = b.admin_email.trim().to_lowercase();
    let mut slug = slugify(&b.tenant_name);
    if store::tenant_slug_taken(&st.pool, &slug).await.map_err(db_err)? {
        slug = format!("{slug}-{}", &Uuid::new_v4().to_string()[..6]);
    }
    let hash = auth::hash_password(&b.admin_password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (tenant_id, _u, _p) =
        store::provision_tenant_with_admin(&st.pool, b.tenant_name.trim(), &slug, &email, Some(&hash))
            .await
            .map_err(db_err)?;
    Ok(Json(tenant_id))
}

#[derive(Serialize)]
struct AssumeResp {
    token: String,
    tenant_id: Uuid,
}

/// Master "navigate as tenant": mint a tenant-scoped token with ALL tenant actions,
/// including `live.view` — while explicitly acting as a tenant the master may watch
/// that tenant's live feeds. (The master's own un-assumed token still cannot: it has
/// no tenant context and the `master` bypass excludes `live.view`.) A future tenant
/// admin approval will gate this assume.
async fn assume_tenant(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<AssumeResp>, ApiErr> {
    require_master(&c)?;
    let tenant = store::find_tenant(&st.pool, id).await.map_err(db_err)?.ok_or(err(StatusCode::NOT_FOUND, "no such tenant"))?;
    let master_uid = c.sub.parse::<Uuid>().ok();
    let perms: Vec<String> = crate::rbac::all_perms().into_iter().map(String::from).collect();
    let claims = Claims {
        sub: c.sub,
        tid: Some(tenant.id.to_string()),
        master: true,
        act_as: true,
        perms,
        exp: auth::expiry_24h(),
    };
    let token = auth::issue(&st.jwt_secret, &claims).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    log::info!("master {} assumed tenant {}", claims.sub, tenant.slug);
    // Record the master session on the tenant's own feed (card #66, Phase 3) — an auditable trace
    // that someone with elevated access entered, distinct from a tenant user's own sign-in.
    let mut ev = app_core::events::NewEvent::new(tenant.id, "auth.login", "system");
    ev.title = Some("Master session".to_string());
    ev.message = Some("A master operator assumed this tenant".to_string());
    ev.user_id = master_uid;
    st.event_queue.push(ev);
    Ok(Json(AssumeResp { token, tenant_id: tenant.id }))
}
