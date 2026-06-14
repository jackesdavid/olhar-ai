//! Live-feed authorization. The gateway is the **enforcement point** for the
//! tenant-private live rule: a client may watch a camera only with a valid control
//! JWT that carries the `live.view` action. A tenant admin's token has it; a master
//! "acting as" a tenant has it (the assume token includes `live.view`); a plain master
//! token (no tenant context) does NOT. The secret is shared with sentinel-control.
use axum::http::{header, HeaderMap};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::Deserialize;

/// The action that gates live feeds.
pub const LIVE_VIEW: &str = "live.view";

/// The subset of control's JWT claims the gateway needs (effective permission set).
/// `exp` is validated by the decoder; `tid` is read once the per-tenant box registry
/// lands (camera-ownership check) — kept here as the claims contract.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Claims {
    #[serde(default)]
    pub tid: Option<String>,
    #[serde(default)]
    pub master: bool,
    #[serde(default)]
    pub act_as: bool,
    #[serde(default)]
    pub perms: Vec<String>,
    pub exp: usize,
}

impl Claims {
    /// Whether this token may watch live feeds. Pure action check — the master's
    /// `live.view` only ever comes from an assume token's explicit `perms` (a plain
    /// master token has an empty `perms`, so this is false for it).
    pub fn may_view_live(&self) -> bool {
        self.perms.iter().any(|p| p == LIVE_VIEW)
    }
}

fn verify(secret: &str, token: &str) -> Option<Claims> {
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default())
        .ok()
        .map(|d| d.claims)
}

/// Pull the bearer token from (in order) the `token` query param, the
/// `Authorization: Bearer` header, or the `sentinel.token` cookie. Browsers can't set
/// headers on `<img>`/WebSocket requests, so the query param is the practical channel.
fn extract_token(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("token=") {
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    if let Some(h) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = h.strip_prefix("Bearer ") {
            return Some(t.to_string());
        }
    }
    if let Some(c) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in c.split(';') {
            if let Some(v) = part.trim().strip_prefix("sentinel.token=") {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// True iff the request carries a valid JWT granting `live.view`. (Per-tenant camera
/// ownership lands with the `tenant_boxes` registry; today there is one upstream.)
pub fn live_authorized(secret: &str, headers: &HeaderMap, query: Option<&str>) -> bool {
    extract_token(headers, query)
        .and_then(|t| verify(secret, &t))
        .map(|c| c.may_view_live())
        .unwrap_or(false)
}
