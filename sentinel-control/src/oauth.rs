//! Social login (Google now; Apple later). Hand-rolled OAuth2 authorization-code flow
//! over reqwest. `/auth/oauth/google/start` → Google consent → `/callback` exchanges
//! the code, reads the user's email, then **finds-or-links-or-signs-up**: first time =
//! a new tenant with that user as Admin (no password — social only). Issues a JWT.
use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Redirect};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{self, Claims};
use crate::{store, AppState};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// Google OAuth config, from env. `redirect_uri` must match a registered URI exactly.
#[derive(Clone)]
pub struct GoogleOauth {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl GoogleOauth {
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("GOOGLE_CLIENT_ID")
            .ok()
            .filter(|s| !s.is_empty())?;
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET")
            .ok()
            .filter(|s| !s.is_empty())?;
        let base =
            std::env::var("OAUTH_REDIRECT_BASE").unwrap_or_else(|_| "http://localhost:8790".into());
        Some(Self {
            client_id,
            client_secret,
            redirect_uri: format!("{base}/auth/oauth/google/callback"),
        })
    }
}

/// Begin the flow: redirect the browser to Google with a CSRF `state` we remember.
pub async fn google_start(State(st): State<AppState>) -> impl IntoResponse {
    let Some(g) = st.google.clone() else {
        return (StatusCode::NOT_IMPLEMENTED, "google login not configured").into_response();
    };
    let state = Uuid::new_v4().to_string();
    st.oauth_states
        .lock()
        .unwrap()
        .insert(state.clone(), Instant::now());
    let enc = urlencoding::encode;
    let url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=online&prompt=select_account",
        enc(&g.client_id),
        enc(&g.redirect_uri),
        enc("openid email profile"),
        enc(&state),
    );
    Redirect::temporary(&url).into_response()
}

#[derive(Deserialize)]
pub struct Callback {
    code: String,
    state: String,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
}

#[derive(Deserialize)]
struct Userinfo {
    sub: String,
    email: String,
}

/// Complete the flow: validate state, exchange the code, read the email, then
/// find/link/sign-up the user and issue a JWT.
pub async fn google_callback(
    State(st): State<AppState>,
    Query(q): Query<Callback>,
) -> impl IntoResponse {
    let Some(g) = st.google.clone() else {
        return (StatusCode::NOT_IMPLEMENTED, "google login not configured").into_response();
    };
    // CSRF: state must be one we issued (and prune old ones).
    {
        let mut states = st.oauth_states.lock().unwrap();
        states.retain(|_, t| t.elapsed().as_secs() < 600);
        if states.remove(&q.state).is_none() {
            return (StatusCode::BAD_REQUEST, "invalid oauth state").into_response();
        }
    }

    let http = reqwest::Client::new();
    let resp = match http
        .post(TOKEN_URL)
        .form(&[
            ("code", q.code.as_str()),
            ("client_id", g.client_id.as_str()),
            ("client_secret", g.client_secret.as_str()),
            ("redirect_uri", g.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("token request failed: {e}"),
            )
                .into_response()
        }
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // Surface Google's exact error (e.g. invalid_client / redirect_uri_mismatch).
        log::error!("google token exchange {status}: {body}");
        return (
            StatusCode::BAD_GATEWAY,
            format!("google token exchange {status}: {body}"),
        )
            .into_response();
    }
    let token: TokenResp = match serde_json::from_str(&body) {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("token parse: {e}")).into_response(),
    };

    let info: Userinfo = match http
        .get(USERINFO_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(i) => i,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, format!("userinfo parse: {e}")).into_response()
            }
        },
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("userinfo failed: {e}")).into_response()
        }
    };

    match resolve_user(&st, &info).await {
        // For the SPA flow, bounce back to the frontend carrying the token in the URL
        // fragment (so it never reaches a server log). Falls back to JSON when no
        // frontend redirect is configured (curl / dev probing).
        Ok(resp) => match std::env::var("OAUTH_SUCCESS_REDIRECT")
            .ok()
            .filter(|s| !s.is_empty())
        {
            Some(base) => {
                let url = format!(
                    "{}#token={}",
                    base.trim_end_matches('/'),
                    urlencoding::encode(&resp.token)
                );
                Redirect::temporary(&url).into_response()
            }
            None => Json(resp).into_response(),
        },
        Err(e) => {
            log::error!("google callback: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "login failed").into_response()
        }
    }
}

#[derive(serde::Serialize)]
struct AuthResp {
    token: String,
    user_id: Uuid,
    tenant_id: Option<Uuid>,
    email: String,
    is_master: bool,
    perms: Vec<String>,
}

/// Find-or-link-or-signup for a verified Google identity, then mint a JWT.
async fn resolve_user(st: &AppState, info: &Userinfo) -> sqlx::Result<AuthResp> {
    let email = info.email.trim().to_lowercase();
    let user_id = if let Some(uid) = store::find_oauth_user(&st.pool, "google", &info.sub).await? {
        uid
    } else if let Some(u) = store::find_user_by_email(&st.pool, &email).await? {
        // Existing email account → link this Google identity to it.
        store::link_oauth(&st.pool, "google", &info.sub, u.id).await?;
        u.id
    } else {
        // First time → create a tenant with this user as Admin (no password).
        let local = email.split('@').next().unwrap_or("workspace");
        let tenant_name = local.to_string();
        let mut slug = slugify(local);
        if store::tenant_slug_taken(&st.pool, &slug).await? {
            slug = format!("{slug}-{}", &Uuid::new_v4().to_string()[..6]);
        }
        let (_tid, uid, _perms) =
            store::provision_tenant_with_admin(&st.pool, &tenant_name, &slug, &email, None).await?;
        store::link_oauth(&st.pool, "google", &info.sub, uid).await?;
        uid
    };

    let user = store::find_user_by_id(&st.pool, user_id)
        .await?
        .expect("user just resolved");
    let perms = if user.is_master {
        Vec::new()
    } else {
        store::effective_perms(&st.pool, user.id).await?
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
    let token = auth::issue(&st.jwt_secret, &claims).unwrap_or_default();
    Ok(AuthResp {
        token,
        user_id: user.id,
        tenant_id: user.tenant_id,
        email: user.email,
        is_master: user.is_master,
        perms,
    })
}

fn slugify(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "workspace".into()
    } else {
        s
    }
}
