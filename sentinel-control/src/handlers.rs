//! HTTP handlers: auth (signup/login/me), RBAC admin (roles, matrix, users), and the
//! master console (tenants + navigate-as-tenant). Every guard is action-based via
//! `auth::require`; the only role-ish gate is the `master` flag.
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
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
        // Self-service password reset (card #166). Both are PUBLIC: a locked-out user has no
        // session to authenticate with, which is the entire point.
        .route(
            "/auth/password-reset/request",
            post(crate::password_reset::request),
        )
        .route(
            "/auth/password-reset/confirm",
            post(crate::password_reset::confirm),
        )
        .route("/auth/me", get(me))
        .route("/auth/media-token", post(media_token))
        .route("/auth/oauth/google/start", get(crate::oauth::google_start))
        .route(
            "/auth/oauth/google/callback",
            get(crate::oauth::google_callback),
        )
        // FIDO2 / WebAuthn passkeys (card #147, §2 E0). register/* + credentials are authenticated
        // (UserClaims); login/* are public.
        .route(
            "/auth/webauthn/register/start",
            post(crate::webauthn::register_start),
        )
        .route(
            "/auth/webauthn/register/finish",
            post(crate::webauthn::register_finish),
        )
        .route(
            "/auth/webauthn/login/start",
            post(crate::webauthn::login_start),
        )
        // Usernameless: no address typed, the assertion says who is signing in.
        .route(
            "/auth/webauthn/login/discoverable/start",
            post(crate::webauthn::discoverable_start),
        )
        .route(
            "/auth/webauthn/login/discoverable/finish",
            post(crate::webauthn::discoverable_finish),
        )
        .route(
            "/auth/webauthn/login/finish",
            post(crate::webauthn::login_finish),
        )
        .route(
            "/auth/webauthn/credentials",
            get(crate::webauthn::list_credentials),
        )
        .route(
            "/auth/webauthn/credentials/:id",
            axum::routing::delete(crate::webauthn::delete_credential),
        )
        .route("/permissions", get(list_permissions))
        .route("/roles", get(list_roles).post(create_role))
        .route(
            "/roles/:id/permissions",
            get(get_role_perms).put(set_role_perms),
        )
        .route("/users", get(list_users).post(invite_user))
        // Invites (#181). Creating one is authenticated; PREVIEWING and ACCEPTING are public —
        // an invitee has no account yet, which is the whole point.
        .route("/users/invites", post(create_invite))
        .route("/auth/invites/:token", get(preview_invite))
        .route("/auth/invites/:token/accept", post(accept_invite))
        .route("/users/:id/roles", put(set_user_roles))
        .route("/users/:id/password", post(set_user_password))
        .route("/users/:id", axum::routing::delete(remove_user))
        // Leaving on your own account — the "different feature" `remove_user` refers to. Required
        // by App Store guideline 5.1.1(v): an app that creates accounts must delete them in-app.
        .route("/auth/me", axum::routing::delete(delete_own_account))
        .route("/tenant/transfer-ownership", post(transfer_ownership))
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
    if s.is_empty() {
        "tenant".into()
    } else {
        s
    }
}

/// Tenant context from the token (set by login/assume). Errors if absent (e.g. the
/// master's own un-assumed token has no tenant).
fn tenant_ctx(c: &Claims) -> Result<Uuid, ApiErr> {
    c.tenant().ok_or(err(
        StatusCode::BAD_REQUEST,
        "no tenant context — assume a tenant first",
    ))
}

fn require_master(c: &Claims) -> Result<(), ApiErr> {
    if c.master {
        Ok(())
    } else {
        Err(err(StatusCode::FORBIDDEN, "master only"))
    }
}

// ---- auth -------------------------------------------------------------------

#[derive(Deserialize)]
struct SignupReq {
    tenant_name: String,
    email: String,
    password: String,
    /// #167 — the user ticked "I agree to the Terms and Privacy Policy".
    ///
    /// `#[serde(default)]` makes a MISSING field `false`, so an older client that does not know
    /// about consent is refused rather than silently recorded as having agreed. Failing closed is
    /// the only safe direction for a legal record.
    #[serde(default)]
    accepted_terms: bool,
    /// UI language at signup (`en` / `pt`), so later emails are written in it. Absent = English,
    /// which is what `Lang::default()` resolves to.
    #[serde(default)]
    locale: Option<String>,
}

/// Public signup: creates a tenant and makes the caller its Admin.
async fn signup(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<SignupReq>,
) -> Result<Json<AuthResp>, ApiErr> {
    // #167. Refuse before touching the database: an account that exists without a recorded
    // acceptance is exactly the state this card was opened to prevent, and it is far easier to
    // never create it than to reconcile it later.
    if !b.accepted_terms {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "you must accept the Terms of Service and Privacy Policy",
        ));
    }
    let email = b.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(err(StatusCode::BAD_REQUEST, "valid email required"));
    }
    if b.password.len() < 8 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters",
        ));
    }
    if store::find_user_by_email(&st.pool, &email)
        .await
        .map_err(db_err)?
        .is_some()
    {
        return Err(err(StatusCode::CONFLICT, "email already registered"));
    }
    // Unique slug (append a short suffix on collision).
    let mut slug = slugify(&b.tenant_name);
    if store::tenant_slug_taken(&st.pool, &slug)
        .await
        .map_err(db_err)?
    {
        slug = format!("{slug}-{}", &Uuid::new_v4().to_string()[..6]);
    }
    let hash =
        auth::hash_password(&b.password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (tenant_id, user_id, perms) = store::provision_tenant_with_admin(
        &st.pool,
        b.tenant_name.trim(),
        &slug,
        &email,
        Some(&hash),
    )
    .await
    .map_err(db_err)?;

    // Remember the language to write to this customer in. A separate UPDATE rather than a new
    // parameter on `provision_tenant_with_admin`, which is shared with the invite and master paths
    // that have no browser to ask. Best-effort: an unset locale falls back to English, and failing
    // a signup over it would be absurd.
    if let Some(tag) = b
        .locale
        .as_deref()
        .map(app_core::notification::templates::Lang::from_tag)
    {
        if let Err(e) = sqlx::query("UPDATE users SET locale = $1 WHERE id = $2")
            .bind(tag.as_tag())
            .bind(user_id)
            .execute(&st.pool)
            .await
        {
            log::warn!("signup: could not store locale for {user_id}: {e}");
        }
    }

    // #167 — evidence of acceptance, recorded against the account that now exists. Runs on the
    // blocking pool like the trial below: `legal::record` drives the Db's own runtime, and calling
    // it directly from this async context would block a Tokio worker.
    {
        let db = st.db.clone();
        let ip = crate::http::enroll_client(&headers);
        let ua = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let _ = tokio::task::spawn_blocking(move || {
            app_core::legal::record_signup_consent(
                &db,
                user_id,
                Some(tenant_id),
                Some(ip.as_str()),
                ua.as_deref(),
            )
        })
        .await;
    }

    // Self-service signup starts the 30-day Beta trial (2 cameras). Best-effort: a failure
    // here must not fail the signup — the master can grant a trial later.
    {
        let db = st.db.clone();
        if let Ok(Err(e)) = tokio::task::spawn_blocking(move || {
            app_core::billing::subscriptions::start_trial(&db, tenant_id)
        })
        .await
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
        scope: None,
        iat: Some(app_core::signing::now_unix() as usize),
    };
    let token = auth::issue(&st.jwt_secret, &claims)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(AuthResp {
        token,
        user_id,
        tenant_id: Some(tenant_id),
        is_master: false,
        perms,
    }))
}

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

async fn login(
    State(st): State<AppState>,
    Json(b): Json<LoginReq>,
) -> Result<Json<AuthResp>, ApiErr> {
    let email = b.email.trim().to_lowercase();
    let Some(user) = store::find_user_by_email(&st.pool, &email)
        .await
        .map_err(db_err)?
    else {
        // Log-only by construction: there is no user, so no tenant to attach an event to. Worth a
        // line anyway — a sweep through addresses that do not exist is what enumeration looks like,
        // and it is the one attack shape the events stream can never show.
        log::warn!("login: attempt for unknown address {}", log_email(&email));
        return Err(err(StatusCode::UNAUTHORIZED, "invalid credentials"));
    };
    // Locked accounts are refused before the password is even considered — otherwise the lockout
    // still leaks whether a guess was right, which is most of what an attacker wanted.
    if locked_now(user.locked_until.as_deref()) {
        // Log, but deliberately do NOT event: this path is reached as often as an attacker cares to
        // keep knocking, so eventing it would hand them a way to flood a tenant's feed — and burn
        // its retention quota — at will. The lock was already evented once, when it engaged.
        log::warn!(
            "login: attempt against locked account {} (user {})",
            log_email(&email),
            user.id
        );
        return Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts — try again later",
        ));
    }
    let Some(hash) = user.password_hash.as_deref() else {
        log::info!(
            "login: password attempt on a social-only account {}",
            log_email(&email)
        );
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "use social login for this account",
        ));
    };
    if !auth::verify_password(&b.password, hash) {
        let failures = user.failed_logins + 1;
        let lock_until = lockout_until(failures);
        store::note_failed_login(&st.pool, user.id, lock_until.clone()).await;
        // PR 2 shipped the lockout with no trace of it ever firing: the failure path bumped a
        // counter and recorded nothing, so neither the tenant nor an operator could see an account
        // being worked on, or answer why someone was locked out. The log line is the forensic trail
        // (it ships to Datadog); the events below are what the tenant should actually see.
        log::warn!(
            "login: failed attempt {failures} for {} (user {})",
            log_email(&email),
            user.id
        );
        // A master account carries no tenant, so it has no feed to land on — the log line above is
        // the whole record for it.
        if let Some(tid) = user.tenant_id {
            // Bounded, which is why this one is safe to event: after LOCKOUT_AFTER failures the
            // account locks and every further attempt short-circuits at the check above without
            // reaching here, so an attacker cannot mint these indefinitely.
            let mut ev = app_core::events::NewEvent::new(tid, "auth.login_failed", "system");
            ev.severity = "warn".to_string();
            ev.title = Some("Failed sign-in".to_string());
            ev.message = Some(format!("Wrong password for {email}"));
            ev.user_id = Some(user.id);
            ev.data = Some(serde_json::json!({ "attempt": failures }));
            st.event_queue.push(ev);
            if let Some(until) = lock_until.as_deref() {
                let mut ev = app_core::events::NewEvent::new(tid, "auth.account_locked", "system");
                ev.severity = "error".to_string();
                ev.title = Some("Account locked".to_string());
                ev.message = Some(format!(
                    "{email} locked after {failures} failed sign-ins — try again after {until}"
                ));
                ev.user_id = Some(user.id);
                ev.data = Some(serde_json::json!({ "attempts": failures, "until": until }));
                st.event_queue.push(ev);
            }
        }
        // Deliberately the same message and status as an unknown address: the throttle must not
        // become an oracle for which accounts exist.
        return Err(err(StatusCode::UNAUTHORIZED, "invalid credentials"));
    }
    if user.failed_logins > 0 {
        store::clear_failed_logins(&st.pool, user.id).await;
    }
    let perms = if user.is_master {
        Vec::new()
    } else {
        store::effective_perms(&st.pool, user.id)
            .await
            .map_err(db_err)?
    };
    let claims = Claims {
        sub: user.id.to_string(),
        tid: user.tenant_id.map(|t| t.to_string()),
        master: user.is_master,
        act_as: false,
        perms: perms.clone(),
        exp: auth::expiry_24h(),
        scope: None,
        iat: Some(app_core::signing::now_unix() as usize),
    };
    let token = auth::issue(&st.jwt_secret, &claims)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // First producer on the agnostic event stream (card #66): record the sign-in. A master token
    // carries no tenant, so only tenant users land on a tenant's feed.
    if let Some(tid) = user.tenant_id {
        let mut ev = app_core::events::NewEvent::new(tid, "auth.login", "user");
        ev.title = Some("Sign-in".to_string());
        ev.message = Some(format!("{} signed in", email));
        ev.user_id = Some(user.id);
        st.event_queue.push(ev);
    }
    Ok(Json(AuthResp {
        token,
        user_id: user.id,
        tenant_id: user.tenant_id,
        is_master: user.is_master,
        perms,
    }))
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

#[derive(Serialize)]
struct MediaTokenResp {
    token: String,
    /// Seconds until it expires — so the caller can re-mint before it does rather than discover it
    /// by failing.
    expires_in: i64,
}

/// `POST /auth/media-token` — mint a short-lived, media-scoped token for URLs that cannot carry an
/// `Authorization` header (`<img src>`, `EventSource`, a WebSocket). Security plan §1.8.
///
/// **No `auth::require` here, deliberately.** The token carries the caller's own identity and its
/// own permissions, just narrowed to `scope: media` and an hour — there is no action being
/// authorized and nothing to escalate to. Gating it on a permission would only mean some users
/// could not view media they are already allowed to view.
///
/// Note what the `UserClaims` extractor buys here for free: it refuses media tokens, so a media
/// token cannot mint another one. Their lifetime is bounded by the session that created them.
async fn media_token(
    State(st): State<AppState>,
    UserClaims(c): UserClaims,
) -> Result<Json<MediaTokenResp>, ApiErr> {
    let media = c.to_media();
    let expires_in = media.exp as i64 - chrono::Utc::now().timestamp();
    let token = auth::issue(&st.jwt_secret, &media)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(MediaTokenResp { token, expires_in }))
}

// ---- tenant owner: reset a member's password --------------------------------

#[derive(Deserialize)]
struct SetPasswordReq {
    password: String,
}

/// Whether `claims` may reset passwords in `tid`.
///
/// The owner always may, **regardless of role grants**. That is the point of having an owner: an
/// Admin can edit the Admin role or drop a role assignment, and without this the tenant could end
/// up with nobody able to restore access to it. Anyone else needs the explicit action.
async fn may_reset_passwords(
    st: &AppState,
    claims: &Claims,
    tid: Uuid,
    caller: Uuid,
) -> Result<bool, ApiErr> {
    if store::tenant_owner(&st.pool, tid).await.map_err(db_err)? == Some(caller) {
        return Ok(true);
    }
    Ok(claims.allows("users.password_reset"))
}

/// Whether `caller` may target `target`, given who owns the tenant.
///
/// The one rule: **only the owner may reset the owner's password.** Without it, this feature
/// creates the escalation it is meant to control — any Admin holding `users.password_reset` could
/// overwrite the owner's password and take the tenant. Everyone else is fair game to a caller who
/// already passed the permission check.
fn may_target(caller: Uuid, target: Uuid, owner: Option<Uuid>) -> bool {
    owner != Some(target) || caller == target
}

/// `POST /users/:id/password` — set another user's password (owner, or `users.password_reset`).
///
/// This is a RESET, not a change: the caller never proves the old password, so it is an account
/// takeover primitive by design and every guard below exists to bound who can point it at whom.
async fn set_user_password(
    State(st): State<AppState>,
    UserClaims(c): UserClaims,
    Path(target_id): Path<Uuid>,
    Json(b): Json<SetPasswordReq>,
) -> Result<StatusCode, ApiErr> {
    let tid = tenant_ctx(&c)?;
    let caller =
        Uuid::parse_str(&c.sub).map_err(|_| err(StatusCode::UNAUTHORIZED, "bad subject"))?;

    if !may_reset_passwords(&st, &c, tid, caller).await? {
        return Err(err(
            StatusCode::FORBIDDEN,
            "missing permission: users.password_reset",
        ));
    }
    // Same floor as signup — a reset must not be a way to set a weaker password than the front door
    // accepts.
    if b.password.len() < 8 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters",
        ));
    }

    let target = store::find_user_by_id(&st.pool, target_id)
        .await
        .map_err(db_err)?
        .ok_or(err(StatusCode::NOT_FOUND, "user not found"))?;

    // Cross-tenant is 404, not 403: 403 would confirm the id exists, turning this into a probe for
    // other tenants' user ids. Checked here AND again in the UPDATE's WHERE clause — the §1.5 bug
    // was exactly an id that got authenticated and then trusted.
    if target.tenant_id != Some(tid) {
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }
    // A master belongs to no tenant and is not a tenant's to reset.
    if target.is_master {
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }
    // Only the owner may reset the OWNER's password. Without this, any Admin holding the action
    // could take over the owner's account and with it the tenant — the escalation this whole
    // feature would otherwise create.
    let owner = store::tenant_owner(&st.pool, tid).await.map_err(db_err)?;
    if !may_target(caller, target_id, owner) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only the owner can reset the owner's password",
        ));
    }

    let hash =
        auth::hash_password(&b.password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let now = app_core::signing::now_unix();
    let n = store::set_password_and_revoke(&st.pool, tid, target_id, &hash, now)
        .await
        .map_err(db_err)?;
    if n == 0 {
        // The guards above passed, so this means the row moved out from under us (deleted, or
        // re-tenanted) between the read and the write. Refuse rather than report success.
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }

    // Audit trail. The password itself is never logged, evented, or returned — only that a reset
    // happened, by whom, to whom.
    log::warn!("password reset: user {target_id} by {caller} (tenant {tid})");
    let mut ev = app_core::events::NewEvent::new(tid, "auth.password_reset", "user");
    ev.severity = "warn".to_string();
    ev.title = Some("Password reset".to_string());
    ev.message = Some(format!("{} had their password reset", target.email));
    ev.user_id = Some(target_id);
    ev.data = Some(serde_json::json!({ "by": caller.to_string() }));
    st.event_queue.push(ev);

    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /users/:id` — remove a member from the tenant (owner, or `users.remove`).
///
/// A hard delete. Their role grants, passkeys and Google links go with them (FK CASCADE), their
/// push devices detach (SET NULL), and their history in the events feed **remains** — `events.user_id`
/// has no foreign key, so removing someone never erases the record of what they did.
async fn remove_user(
    State(st): State<AppState>,
    UserClaims(c): UserClaims,
    Path(target_id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let tid = tenant_ctx(&c)?;
    let caller =
        Uuid::parse_str(&c.sub).map_err(|_| err(StatusCode::UNAUTHORIZED, "bad subject"))?;

    let owner = store::tenant_owner(&st.pool, tid).await.map_err(db_err)?;
    let permitted = owner == Some(caller) || c.allows("users.remove");
    if !permitted {
        return Err(err(
            StatusCode::FORBIDDEN,
            "missing permission: users.remove",
        ));
    }

    // Removing yourself is always a mistake here — it is either a misclick or the last admin
    // locking the tenant out of its own user management. Leaving is a different feature.
    if caller == target_id {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "you cannot remove your own account",
        ));
    }
    // The owner cannot be removed by ANYONE, including themselves. `tenants.owner_user_id` is
    // ON DELETE SET NULL, so deleting an owner would silently leave the tenant ownerless — the
    // exact unrecoverable state the owner concept exists to prevent. Transfer ownership first.
    if owner == Some(target_id) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "the tenant owner cannot be removed — transfer ownership first",
        ));
    }

    // Read before delete purely for the audit line; the DELETE re-checks every condition itself.
    let target = store::find_user_by_id(&st.pool, target_id)
        .await
        .map_err(db_err)?
        .ok_or(err(StatusCode::NOT_FOUND, "user not found"))?;
    // Cross-tenant and master are 404, not 403 — same reasoning as the password reset: a 403 would
    // confirm the id exists.
    if target.tenant_id != Some(tid) || target.is_master {
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }

    let n = store::remove_tenant_user(&st.pool, tid, target_id)
        .await
        .map_err(db_err)?;
    if n == 0 {
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }

    log::warn!("user removed: {target_id} by {caller} (tenant {tid})");
    let mut ev = app_core::events::NewEvent::new(tid, "auth.user_removed", "user");
    ev.severity = "warn".to_string();
    ev.title = Some("User removed".to_string());
    ev.message = Some(format!("{} was removed from the tenant", target.email));
    ev.user_id = Some(target_id);
    ev.data = Some(serde_json::json!({ "by": caller.to_string() }));
    st.event_queue.push(ev);

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DeleteMeReq {
    /// Required when the account HAS a password. Absent for social-only accounts, which have
    /// nothing to check — refusing those would make the account undeletable, which is the state
    /// this endpoint exists to prevent.
    #[serde(default)]
    password: Option<String>,
}

/// `DELETE /auth/me` — delete your OWN account.
///
/// This deletes the person, not the organisation. The tenant, its cameras, workflows, recordings
/// and every other member carry on exactly as before: an account is a login, and the product it
/// logs into belongs to the tenant. Apple requires this (5.1.1(v)) because the app creates
/// accounts — which it does since invites shipped — and `remove_user` deliberately refuses
/// self-removal, naming this as the separate feature it is.
///
/// What goes: the user row, their role grants, passkeys and Google links (FK CASCADE), and their
/// push devices detach (SET NULL) so no notification is ever routed at a deleted person. What
/// stays: their history in the events feed. `events.user_id` has no foreign key precisely so that
/// leaving never erases the record of what was done.
async fn delete_own_account(
    State(st): State<AppState>,
    UserClaims(c): UserClaims,
    Json(b): Json<DeleteMeReq>,
) -> Result<StatusCode, ApiErr> {
    let tid = tenant_ctx(&c)?;
    let me = Uuid::parse_str(&c.sub).map_err(|_| err(StatusCode::UNAUTHORIZED, "bad subject"))?;
    let user = store::find_user_by_id(&st.pool, me)
        .await
        .map_err(db_err)?
        .ok_or(err(StatusCode::NOT_FOUND, "user not found"))?;
    if user.is_master {
        return Err(err(
            StatusCode::FORBIDDEN,
            "a master account is not a tenant member",
        ));
    }

    // The owner cannot delete themselves, for the same reason nobody else can:
    // `tenants.owner_user_id` is ON DELETE SET NULL, so it would leave the tenant ownerless and
    // unrecoverable. Apple accepts a blocker like this as long as it is ACTIONABLE in the app —
    // transferring ownership is, and the message says so rather than just refusing.
    let owner = store::tenant_owner(&st.pool, tid).await.map_err(db_err)?;
    if owner == Some(me) {
        return Err(err(
            StatusCode::CONFLICT,
            "you own this workspace — transfer ownership to someone else first, then delete your account",
        ));
    }

    // A password re-check, when there is one to check. An unlocked phone in the wrong hands should
    // not be one tap away from destroying an account.
    if let Some(hash) = user.password_hash.as_deref() {
        let given = b.password.unwrap_or_default();
        if given.is_empty() || !auth::verify_password(&given, hash) {
            return Err(err(StatusCode::UNAUTHORIZED, "wrong password"));
        }
    }

    let n = store::remove_tenant_user(&st.pool, tid, me)
        .await
        .map_err(db_err)?;
    if n == 0 {
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }

    log::warn!("user deleted their own account: {me} (tenant {tid})");
    let mut ev = app_core::events::NewEvent::new(tid, "auth.user_left", "user");
    ev.severity = "warn".to_string();
    ev.title = Some("Account deleted".to_string());
    ev.message = Some(format!("{} deleted their own account", user.email));
    ev.user_id = Some(me);
    st.event_queue.push(ev);

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TransferOwnershipReq {
    user_id: Uuid,
}

/// `POST /tenant/transfer-ownership` — hand the tenant to another member.
///
/// **Only the current owner may call this, and it is deliberately not delegatable.** Every other
/// owner power can be granted to a role; this one cannot, because an action that can hand the
/// tenant away is not something to hold accidentally. It is also the escape hatch the other two
/// guards point at ("transfer ownership first"), so it must not itself depend on a role grant that
/// could have been edited away.
///
/// Ownership takes effect immediately — the handlers read `tenants.owner_user_id` on every call
/// rather than trusting the token — so unlike a role change this does not wait for re-login. The
/// previous owner loses owner powers on their very next request.
async fn transfer_ownership(
    State(st): State<AppState>,
    UserClaims(c): UserClaims,
    Json(b): Json<TransferOwnershipReq>,
) -> Result<StatusCode, ApiErr> {
    let tid = tenant_ctx(&c)?;
    let caller =
        Uuid::parse_str(&c.sub).map_err(|_| err(StatusCode::UNAUTHORIZED, "bad subject"))?;

    let owner = store::tenant_owner(&st.pool, tid).await.map_err(db_err)?;
    if owner != Some(caller) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "only the current owner can transfer ownership",
        ));
    }
    if b.user_id == caller {
        return Err(err(StatusCode::BAD_REQUEST, "you already own this tenant"));
    }

    let target = store::find_user_by_id(&st.pool, b.user_id)
        .await
        .map_err(db_err)?
        .ok_or(err(StatusCode::NOT_FOUND, "user not found"))?;
    // Same 404-not-403 reasoning as the other two: a 403 would confirm the id exists.
    if target.tenant_id != Some(tid) || target.is_master {
        return Err(err(StatusCode::NOT_FOUND, "user not found"));
    }
    // A tenant whose owner cannot sign in is a tenant with no recovery path. An OAuth-only account
    // is fine (it has a way in); an account with neither a password nor a social identity is not.
    if target.password_hash.is_none()
        && !store::has_oauth_identity(&st.pool, b.user_id)
            .await
            .map_err(db_err)?
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "that account has no way to sign in — set a password for them first",
        ));
    }

    store::set_tenant_owner(&st.pool, tid, b.user_id)
        .await
        .map_err(db_err)?;

    log::warn!(
        "ownership transferred: tenant {tid} from {caller} to {}",
        b.user_id
    );
    let mut ev = app_core::events::NewEvent::new(tid, "auth.ownership_transferred", "user");
    ev.severity = "warn".to_string();
    ev.title = Some("Ownership transferred".to_string());
    ev.message = Some(format!("{} is now the tenant owner", target.email));
    ev.user_id = Some(b.user_id);
    ev.data = Some(serde_json::json!({ "from": caller.to_string() }));
    st.event_queue.push(ev);

    Ok(StatusCode::NO_CONTENT)
}

// ---- RBAC -------------------------------------------------------------------

async fn list_permissions(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let perms = store::list_permissions(&st.pool).await.map_err(db_err)?;
    Ok(Json(
        perms
            .into_iter()
            .map(|(key, description)| serde_json::json!({ "key": key, "description": description }))
            .collect(),
    ))
}

async fn list_roles(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<store::RoleRow>>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    Ok(Json(
        store::list_roles(&st.pool, tid).await.map_err(db_err)?,
    ))
}

#[derive(Deserialize)]
struct CreateRoleReq {
    name: String,
}

async fn create_role(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<CreateRoleReq>,
) -> Result<Json<Uuid>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "role name required"));
    }
    Ok(Json(
        store::create_role(&st.pool, tid, name, false)
            .await
            .map_err(db_err)?,
    ))
}

async fn get_role_perms(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<String>>, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    // Answer the same way as a write: a role from another tenant simply doesn't exist here.
    if !store::role_in_tenant(&st.pool, tid, id)
        .await
        .map_err(db_err)?
    {
        return Err(err(StatusCode::NOT_FOUND, "role not found"));
    }
    Ok(Json(
        store::role_perm_keys(&st.pool, tid, id)
            .await
            .map_err(db_err)?,
    ))
}

#[derive(Deserialize)]
struct SetPermsReq {
    keys: Vec<String>,
}

async fn set_role_perms(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<SetPermsReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "roles.manage")?;
    let tid = tenant_ctx(&c)?;
    // The role id comes from the client. Refuse a role from another tenant outright rather
    // than letting the scoped writes silently no-op.
    if !store::role_in_tenant(&st.pool, tid, id)
        .await
        .map_err(db_err)?
    {
        return Err(err(StatusCode::NOT_FOUND, "role not found"));
    }
    // Unknown keys would silently vanish (the FK drops them); reject so the UI can't drift
    // from the catalog.
    for k in &b.keys {
        if !crate::rbac::all_perms().contains(&k.as_str()) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                format!("unknown permission '{k}'"),
            ));
        }
    }
    store::set_role_permissions(&st.pool, tid, id, &b.keys)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<store::UserListRow>>, ApiErr> {
    let tid = tenant_ctx(&c)?;
    // Owner OR users.manage. The owner's actions all live on this screen, so gating it purely on a
    // role grant would let a role edit hide the tenant's own recovery path from the one person who
    // cannot be locked out of it.
    let caller =
        Uuid::parse_str(&c.sub).map_err(|_| err(StatusCode::UNAUTHORIZED, "bad subject"))?;
    if store::tenant_owner(&st.pool, tid).await.map_err(db_err)? != Some(caller) {
        auth::require(&c, "users.manage")?;
    }
    Ok(Json(
        store::list_users(&st.pool, tid).await.map_err(db_err)?,
    ))
}

#[derive(Deserialize)]
struct InviteReq {
    email: String,
    password: String,
    role_ids: Vec<Uuid>,
}

async fn invite_user(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<InviteReq>,
) -> Result<Json<Uuid>, ApiErr> {
    auth::require(&c, "users.manage")?;
    let tid = tenant_ctx(&c)?;
    let email = b.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(err(StatusCode::BAD_REQUEST, "valid email required"));
    }
    if b.password.len() < 8 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters",
        ));
    }
    let hash =
        auth::hash_password(&b.password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let uid = store::create_user(&st.pool, Some(tid), &email, Some(&hash), false)
        .await
        .map_err(db_err)?;
    for r in b.role_ids {
        store::grant_role(&st.pool, uid, r).await.map_err(db_err)?;
    }
    Ok(Json(uid))
}

#[derive(Deserialize)]
struct CreateInviteReq {
    /// Optional: a QR handed over in person needs no address, and demanding one invents a fake.
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    role_ids: Vec<Uuid>,
    /// Days the invite stays open. Clamped in `invites::mint`.
    #[serde(default)]
    expires_days: Option<i64>,
}

/// Mint an invite. Returns the raw token + typed code EXACTLY once — they are not stored in the
/// clear and cannot be re-read, so the UI must show them now (#181).
async fn create_invite(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<CreateInviteReq>,
) -> Result<Json<app_core::invites::MintedInvite>, ApiErr> {
    auth::require(&c, "users.manage")?;
    let tid = tenant_ctx(&c)?;
    let by = c.sub.parse::<Uuid>().ok();
    let email = b
        .email
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty());
    if let Some(e) = &email {
        if !e.contains('@') {
            return Err(err(StatusCode::BAD_REQUEST, "valid email required"));
        }
    }
    let ttl = b.expires_days.unwrap_or(7).clamp(1, 30) * 24 * 3600;
    let db = st.db.clone();
    let minted = crate::domain::blocking(move || {
        app_core::invites::mint(&db, tid, email.as_deref(), &b.role_ids, ttl, by)
    })
    .await?
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(minted))
}

/// What an unauthenticated holder of an invite may see: which organisation it is for, so the app
/// can ask "Join Acme?" before anyone types anything. Deliberately not the roles or any id.
///
/// A missing, expired or already-redeemed invite all answer 404 identically — this endpoint is
/// enumerable, and distinguishing them would say which tokens exist.
async fn preview_invite(
    State(st): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<app_core::invites::InvitePreview>, ApiErr> {
    let db = st.db.clone();
    let p = crate::domain::blocking(move || app_core::invites::preview(&db, &token)).await?;
    p.map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "this invite is not valid any more"))
}

#[derive(Deserialize)]
struct AcceptInviteReq {
    /// Required when the invite carries no address (a QR handed over in person).
    #[serde(default)]
    email: Option<String>,
    password: String,
}

/// Redeem an invite: create the user, grant the baked-in roles, and mint the SAME session shape
/// `/auth/login` returns — so the app's existing session handling is untouched and the phone is
/// usable before any second step.
async fn accept_invite(
    State(st): State<AppState>,
    Path(token): Path<String>,
    Json(b): Json<AcceptInviteReq>,
) -> Result<Json<AuthResp>, ApiErr> {
    use app_core::invites::AcceptError;
    if b.password.len() < 8 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters",
        ));
    }
    let db = st.db.clone();
    let claimed = crate::domain::blocking(move || app_core::invites::claim(&db, &token))
        .await?
        .map_err(|e| match e {
            AcceptError::NotFound => err(
                StatusCode::NOT_FOUND,
                "this invite is not valid any more — ask for a new one",
            ),
            AcceptError::Expired => err(
                StatusCode::GONE,
                "this invite has expired — ask for a new one",
            ),
            AcceptError::EmailTaken => err(StatusCode::CONFLICT, "that address is already in use"),
            AcceptError::Db(m) => err(StatusCode::INTERNAL_SERVER_ERROR, m),
        })?;

    let email = claimed
        .email
        .clone()
        .or_else(|| b.email.map(|e| e.trim().to_lowercase()))
        .filter(|e| e.contains('@'))
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "valid email required"))?;

    let hash =
        auth::hash_password(&b.password).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let uid = store::create_user(
        &st.pool,
        Some(claimed.tenant_id),
        &email,
        Some(&hash),
        false,
    )
    .await
    .map_err(db_err)?;
    for r in claimed.role_ids {
        store::grant_role(&st.pool, uid, r).await.map_err(db_err)?;
    }
    let db = st.db.clone();
    let invite_id = claimed.id;
    let _ = crate::domain::blocking(move || {
        app_core::invites::note_accepted_user(&db, invite_id, uid);
    })
    .await;

    let perms = store::effective_perms(&st.pool, uid)
        .await
        .map_err(db_err)?;
    let claims = Claims {
        sub: uid.to_string(),
        tid: Some(claimed.tenant_id.to_string()),
        master: false,
        act_as: false,
        perms: perms.clone(),
        exp: auth::expiry_24h(),
        scope: None,
        iat: Some(app_core::signing::now_unix() as usize),
    };
    let token = auth::issue(&st.jwt_secret, &claims)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut ev = app_core::events::NewEvent::new(claimed.tenant_id, "auth.invite_accepted", "user");
    ev.title = Some("Joined the team".to_string());
    ev.message = Some(format!("{email} accepted an invitation"));
    ev.user_id = Some(uid);
    st.event_queue.push(ev);
    Ok(Json(AuthResp {
        token,
        user_id: uid,
        tenant_id: Some(claimed.tenant_id),
        is_master: false,
        perms,
    }))
}

#[derive(Deserialize)]
struct SetUserRolesReq {
    role_ids: Vec<Uuid>,
}

async fn set_user_roles(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<SetUserRolesReq>,
) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "users.manage")?;
    // replace-all: clear then grant
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
        .bind(id)
        .execute(&st.pool)
        .await
        .map_err(db_err)?;
    for r in b.role_ids {
        store::grant_role(&st.pool, id, r).await.map_err(db_err)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- master console ---------------------------------------------------------

async fn list_tenants(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
) -> Result<Json<Vec<store::TenantRow>>, ApiErr> {
    require_master(&c)?;
    Ok(Json(store::list_tenants(&st.pool).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct CreateTenantReq {
    tenant_name: String,
    admin_email: String,
    admin_password: String,
}

async fn create_tenant(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Json(b): Json<CreateTenantReq>,
) -> Result<Json<Uuid>, ApiErr> {
    require_master(&c)?;
    let email = b.admin_email.trim().to_lowercase();
    let mut slug = slugify(&b.tenant_name);
    if store::tenant_slug_taken(&st.pool, &slug)
        .await
        .map_err(db_err)?
    {
        slug = format!("{slug}-{}", &Uuid::new_v4().to_string()[..6]);
    }
    let hash = auth::hash_password(&b.admin_password)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (tenant_id, _u, _p) = store::provision_tenant_with_admin(
        &st.pool,
        b.tenant_name.trim(),
        &slug,
        &email,
        Some(&hash),
    )
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
async fn assume_tenant(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AssumeResp>, ApiErr> {
    require_master(&c)?;
    let tenant = store::find_tenant(&st.pool, id)
        .await
        .map_err(db_err)?
        .ok_or(err(StatusCode::NOT_FOUND, "no such tenant"))?;
    let master_uid = c.sub.parse::<Uuid>().ok();
    let perms: Vec<String> = crate::rbac::all_perms()
        .into_iter()
        .map(String::from)
        .collect();
    let claims = Claims {
        sub: c.sub,
        tid: Some(tenant.id.to_string()),
        master: true,
        act_as: true,
        perms,
        exp: auth::expiry_24h(),
        scope: None,
        iat: Some(app_core::signing::now_unix() as usize),
    };
    let token = auth::issue(&st.jwt_secret, &claims)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    log::info!("master {} assumed tenant {}", claims.sub, tenant.slug);
    // Record the master session on the tenant's own feed (card #66, Phase 3) — an auditable trace
    // that someone with elevated access entered, distinct from a tenant user's own sign-in.
    let mut ev = app_core::events::NewEvent::new(tenant.id, "auth.login", "system");
    ev.title = Some("Master session".to_string());
    ev.message = Some("A master operator assumed this tenant".to_string());
    ev.user_id = master_uid;
    st.event_queue.push(ev);
    Ok(Json(AssumeResp {
        token,
        tenant_id: tenant.id,
    }))
}

// ---- login throttling -------------------------------------------------------

/// An address rendered safe to put in a log line.
///
/// The address on a failed sign-in is attacker-controlled, and both of its abuses are cheap.
/// `{:?}` quotes and escapes, so an embedded newline cannot forge a second log line (nor a fake
/// one claiming a successful sign-in); the cap stops a megabyte of submitted text becoming a
/// megabyte of log, billed and shipped. Truncation counts CHARS, not bytes — slicing a non-ASCII
/// address mid-codepoint would panic, and panicking inside the login path is a denial of service
/// reachable by anyone who can type a UTF-8 address into a form.
fn log_email(email: &str) -> String {
    const MAX_CHARS: usize = 120;
    let clipped: String = email.chars().take(MAX_CHARS).collect();
    if clipped.len() < email.len() {
        format!("{clipped:?}…")
    } else {
        format!("{clipped:?}")
    }
}

/// Failures tolerated before the account is locked at all.
///
/// Generous on purpose: a real person mistyping twice must not be locked out, and the goal is to
/// make guessing uneconomic rather than to punish fumbling. The escalation below does the work.
const LOCKOUT_AFTER: i32 = 5;

/// How long an account is locked after `failures` consecutive failed sign-ins.
///
/// Escalating, and capped at an hour. The cap matters: an unbounded backoff hands an attacker a
/// denial-of-service — anyone who knows an address could lock its owner out permanently by failing
/// on purpose. An hour is long enough to make guessing hopeless (a few attempts per hour against a
/// real password) and short enough that a locked-out user is inconvenienced, not stranded.
fn lockout_until(failures: i32) -> Option<String> {
    if failures < LOCKOUT_AFTER {
        return None;
    }
    let secs = match failures {
        5..=6 => 60,
        7..=9 => 300,
        _ => 3600,
    };
    Some(app_core::timeutil::plus_secs_rfc3339(secs))
}

/// Whether a stored `locked_until` is still in the future.
///
/// Parsed rather than compared as text, unlike most timestamp checks here. Textual comparison is
/// sound only for two values that are both valid RFC3339 from the same clock, and this one is a
/// gate on someone's ability to sign in: any string that is not a timestamp sorts above `"2026-…"`
/// — `"not-a-timestamp"` starts with `n` — and would lock the account out permanently. Parsing
/// makes a corrupt value fail open, which is the right direction for a throttle. The counter still
/// escalates, so this is not a way around the lockout.
fn locked_now(locked_until: Option<&str>) -> bool {
    let Some(t) = locked_until.map(str::trim).filter(|t| !t.is_empty()) else {
        return false;
    };
    match chrono::DateTime::parse_from_rfc3339(t) {
        Ok(until) => until > chrono::Local::now(),
        Err(_) => {
            log::warn!("login: unparseable locked_until {t:?} — treating the account as unlocked");
            false
        }
    }
}

#[cfg(test)]
mod lockout_tests {
    use super::*;

    /// Ordinary mistyping must not lock a real person out.
    #[test]
    fn a_few_failures_do_not_lock_the_account() {
        for n in 1..LOCKOUT_AFTER {
            assert_eq!(lockout_until(n), None, "{n} failures should not lock");
        }
        assert!(lockout_until(LOCKOUT_AFTER).is_some());
    }

    /// The lock must escalate — a fixed short delay barely slows an automated guesser.
    #[test]
    fn repeated_failures_escalate() {
        let at = |n| lockout_until(n).unwrap();
        assert!(at(5) < at(8), "5 failures must unlock sooner than 8");
        assert!(at(8) < at(20), "8 failures must unlock sooner than 20");
    }

    /// The escalation is capped, or knowing someone's email is enough to lock them out forever.
    #[test]
    fn the_lockout_is_capped_so_it_cannot_become_a_denial_of_service() {
        let hour_ish = app_core::timeutil::plus_secs_rfc3339(3700);
        assert!(
            lockout_until(10_000).unwrap() < hour_ish,
            "an unbounded backoff would let anyone lock any account permanently"
        );
    }

    #[test]
    fn a_future_lock_holds_and_a_past_one_does_not() {
        assert!(locked_now(Some(&app_core::timeutil::plus_secs_rfc3339(60))));
        assert!(!locked_now(Some(&app_core::timeutil::plus_secs_rfc3339(
            -60
        ))));
        assert!(!locked_now(None));
        assert!(!locked_now(Some("")));
    }

    /// A corrupt value must fail open, not strand the user forever.
    #[test]
    fn an_unparseable_lock_does_not_strand_the_user() {
        assert!(!locked_now(Some("not-a-timestamp")));
    }
}

#[cfg(test)]
mod failed_login_record_tests {
    use super::*;

    /// Truncating by BYTES would panic on this input, and a panic inside the login path is a denial
    /// of service anyone can reach by typing a non-ASCII address into the form. Byte 120 of this
    /// string falls inside a 3-byte character on purpose.
    #[test]
    fn a_long_multibyte_address_is_clipped_without_panicking() {
        let email = format!("ab{}", "日".repeat(200));
        let out = log_email(&email);
        assert!(out.ends_with('…'), "expected a truncation marker: {out}");
        assert!(out.chars().count() < email.chars().count());
    }

    /// The address is attacker-controlled, so it must not be able to forge a second log line —
    /// least of all one that reads like a successful sign-in.
    #[test]
    fn a_newline_in_the_address_cannot_forge_a_log_line() {
        let out = log_email("a@b.c\nlogin: signed in as admin");
        assert!(!out.contains('\n'), "a raw newline reached the log: {out}");
        assert!(out.contains("\\n"), "the newline should be escaped: {out}");
    }

    /// The ordinary case has to stay readable, or the logging is useless to whoever reads it.
    #[test]
    fn an_ordinary_address_is_quoted_and_otherwise_intact() {
        assert_eq!(log_email("someone@example.com"), "\"someone@example.com\"");
    }

    /// An event kind the feed cannot label still stores fine — and then shows up blank in the UI,
    /// which is how a typo here stays invisible. Keeps the producer and the catalog honest.
    #[test]
    fn the_auth_kinds_this_module_emits_are_in_the_feed_catalog() {
        let known: Vec<&str> = app_core::events::catalog().iter().map(|k| k.kind).collect();
        for kind in [
            "auth.login",
            "auth.login_failed",
            "auth.account_locked",
            "auth.password_reset",
            "auth.user_removed",
            "auth.ownership_transferred",
        ] {
            assert!(
                known.contains(&kind),
                "{kind} is emitted by handlers::login but missing from events::catalog()"
            );
        }
    }
}

#[cfg(test)]
mod password_reset_tests {
    use super::*;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    /// The escalation this rule exists to stop: an Admin with the action must not be able to
    /// overwrite the owner's password and take the tenant.
    #[test]
    fn a_non_owner_cannot_target_the_owner() {
        let (admin, owner) = (id(1), id(2));
        assert!(!may_target(admin, owner, Some(owner)));
    }

    /// The owner is not locked out of their own account by that rule.
    #[test]
    fn the_owner_can_target_themselves() {
        let owner = id(2);
        assert!(may_target(owner, owner, Some(owner)));
    }

    /// Ordinary members stay resettable by anyone who passed the permission check.
    #[test]
    fn anyone_permitted_can_target_an_ordinary_member() {
        let (admin, member, owner) = (id(1), id(3), id(2));
        assert!(may_target(admin, member, Some(owner)));
    }

    /// An ownerless tenant (migration 0081 leaves these NULL rather than guessing) must not become
    /// un-administrable — with no owner there is no owner to protect.
    #[test]
    fn an_ownerless_tenant_has_nobody_to_protect() {
        assert!(may_target(id(1), id(3), None));
        assert!(may_target(id(1), id(2), None));
    }
}
