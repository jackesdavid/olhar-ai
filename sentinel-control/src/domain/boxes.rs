//! Edge appliance ("box") registry: a tenant admin creates boxes and gets a one-time
//! box token to put in the edge installer. The edge presents that token to
//! `/config/cameras` (see `http.rs`) and control returns only that box's tenant's
//! cameras. `/internal/box-upstream` lets the gateway resolve a tenant's live host.
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{err, tenant_of, ApiErr};
use crate::auth::{self, UserClaims};
use crate::{store, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/boxes", get(list).post(create))
        .route("/boxes/:id", axum::routing::delete(remove))
        .route("/boxes/:id/restart", axum::routing::post(restart))
        .route("/boxes/:id/update", axum::routing::post(update))
        .route("/boxes/:id/logs", get(box_logs))
        .route("/boxes/:id/logs/stream", get(box_logs_stream))
        .route("/internal/box-upstream", get(box_upstream))
        .route("/boxes/:id/recording-config", axum::routing::patch(patch_recording_config))
        .route("/boxes/:id/tracking-config", axum::routing::patch(patch_tracking_config))
        .route("/boxes/:id/storage-stats", get(storage_stats))
        .route("/boxes/:id/pairing-code", axum::routing::post(pairing_code))
}

/// How long a pairing code is good for. A code that hasn't been typed in fifteen minutes is a code
/// someone left on a screen and walked away from.
pub const PAIRING_TTL_SECS: i64 = 15 * 60;

/// **Crockford base32.** `I`, `L`, `O` and `U` are absent on purpose: the first three get misread
/// off a screen (as 1, 1, 0) and the fourth was dropped by Crockford to avoid spelling accidents.
/// Decoding is forgiving about exactly those confusions — see [`normalize_code`].
const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A pairing code: 10 symbols of Crockford base32 ≈ 32^10 ≈ 1.1e15 — hopeless to guess inside a
/// 15-minute window, especially against the rate limiter on the redeem path. Rendered `ABCDE-FGHIJ`
/// because a human has to read it off one screen and type it into another.
fn new_pairing_code() -> String {
    let bytes = *Uuid::new_v4().as_bytes();
    let pick = |i: usize| char::from(CODE_ALPHABET[usize::from(bytes[i]) % CODE_ALPHABET.len()]);
    let s: String = (0..10).map(pick).collect();
    format!("{}-{}", &s[..5], &s[5..])
}

/// Undo the ways a human mistypes a code: case, the dash, stray spaces, and Crockford's own
/// confusable set (`O`→`0`, `I`/`L`→`1`). Applied on **both** sides — mint hashes the canonical
/// form, redeem normalizes before hashing — so `abcde-fghij` and `ABCDEFGHIJ` are the same code.
pub fn normalize_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

#[derive(Serialize)]
struct PairingResp {
    /// Shown once, in the browser. Only its hash is stored.
    code: String,
    expires_at: String,
}

/// Mint a pairing code for a box. The code is the ONLY thing the person at the appliance needs —
/// it redeems (once, at `POST /enroll`) into a freshly-minted box token + Tailscale key.
async fn pairing_code(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PairingResp>, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;

    // Prove the box is this tenant's before minting anything for it.
    if store::box_name(&st.pool, id).await.map_err(db_err)?.is_none()
        || !store::list_boxes(&st.pool, tid).await.map_err(db_err)?.iter().any(|b| b.id == id)
    {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }

    let code = new_pairing_code();
    let expires_at = app_core::timeutil::plus_secs_rfc3339(PAIRING_TTL_SECS);
    store::create_pairing(&st.pool, tid, id, &hash_token(&normalize_code(&code)), &expires_at)
        .await
        .map_err(db_err)?;

    Ok(Json(PairingResp { code, expires_at }))
}

fn db_err(e: sqlx::Error) -> ApiErr {
    log::error!("boxes db error: {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

/// sha256(token) as hex — the deterministic form stored + looked up. Public so the
/// camera-config endpoint hashes the presented token the same way.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// A fresh high-entropy box token (`sbx_<32 hex><32 hex>`). Shown once — at box creation, or handed
/// straight to the box when it redeems a pairing code (`http::enroll`).
pub fn new_box_token() -> String {
    format!("sbx_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn list(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<Vec<store::BoxRow>>, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    Ok(Json(store::list_boxes(&st.pool, tid).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct CreateReq {
    name: String,
}

#[derive(Serialize)]
struct CreateResp {
    id: Uuid,
    /// The plaintext box token — shown ONCE; the installer needs it as BOX_TOKEN.
    token: String,
    /// A unique tailnet hostname for this box (`--hostname`).
    hostname: String,
    /// A freshly-minted, transparent Tailscale auth key (`--authkey`) when the
    /// enrollment broker is configured — so the tenant never logs into Tailscale.
    /// Null when not configured (the UI then asks for a key).
    tailscale_key: Option<String>,
}

async fn create(UserClaims(c): UserClaims, State(st): State<AppState>, Json(b): Json<CreateReq>) -> Result<Json<CreateResp>, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "box name required"));
    }
    let token = new_box_token();
    let id = store::create_box(&st.pool, tid, name, &hash_token(&token)).await.map_err(db_err)?;
    let hostname = format!("sentinel-{}", &id.to_string()[..8]);

    // Transparent enrollment: mint a Tailscale auth key for this box (best-effort —
    // if the broker isn't configured or the API fails, fall back to a manual key).
    let tailscale_key = match &st.tailscale {
        Some(ts) => match ts.mint_auth_key(&st.http, &format!("sentinel-edge {name}")).await {
            Ok(k) => Some(k),
            Err(e) => {
                log::warn!("tailscale key mint failed for box {id}: {e}");
                None
            }
        },
        None => None,
    };

    Ok(Json(CreateResp { id, token, hostname, tailscale_key }))
}

async fn remove(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    if store::delete_box(&st.pool, tid, id).await.map_err(db_err)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "box not found"))
    }
}

/// Queue a restart for a box: the edge picks it up on its next poll and exits, so the
/// service manager relaunches it (re-running `tailscale up` + re-serving go2rtc).
async fn restart(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    st.request_restart(id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Queue an update for a box: the edge exits on its next poll, the service manager
/// relaunches it, and `maybe_self_update()` at startup downloads + installs the new binary.
async fn update(UserClaims(c): UserClaims, State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiErr> {
    auth::require(&c, "devices.write")?;
    let tid = tenant_of(&c)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    st.request_update(id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct UpstreamResp {
    /// The tenant's live go2rtc host (tailnet MagicDNS), or null if no box reported one.
    host: Option<String>,
}

/// The gateway calls this (forwarding the user's JWT) to resolve the caller's tenant's
/// live upstream. Any authenticated user with a tenant context may resolve their own.
async fn box_upstream(UserClaims(c): UserClaims, State(st): State<AppState>) -> Result<Json<UpstreamResp>, ApiErr> {
    let tid = tenant_of(&c)?;
    Ok(Json(UpstreamResp { host: store::box_host_for_tenant(&st.pool, tid).await.map_err(db_err)? }))
}

// ---- per-box logs -----------------------------------------------------------

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<i64>,
}

/// The recent persisted log tail for a box (oldest-first), so the viewer has history on open.
async fn box_logs(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<Vec<store::BoxLogRow>>, ApiErr> {
    auth::require(&c, "devices.read")?;
    let tid = tenant_of(&c)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(store::list_box_logs(&st.pool, tid, id, limit).await.map_err(db_err)?))
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

/// Live SSE tail of a box's logs, scoped to the caller's tenant + this box. JWT rides the
/// `token` query param (EventSource can't set headers). Subscribes to the control-wide
/// `log_bus` (fed by the edge's /config/logs pushes) and forwards only matching lines.
async fn box_logs_stream(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TokenQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let claims = auth::verify(&st.jwt_secret, &q.token).map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    auth::require(&claims, "devices.read")?;
    let tid = tenant_of(&claims)?;
    if !store::box_belongs_to(&st.pool, tid, id).await.map_err(db_err)? {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    let tid_s = tid.to_string();
    let box_s = id.to_string();

    let mut rx = st.log_bus.subscribe();
    let (tx, out) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(256);
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(line) => {
                    let matches = serde_json::from_str::<Value>(&line)
                        .ok()
                        .map(|v| {
                            v.get("tenant").and_then(|t| t.as_str()) == Some(tid_s.as_str())
                                && v.get("box").and_then(|b| b.as_str()) == Some(box_s.as_str())
                        })
                        .unwrap_or(false);
                    if matches && tx.send(Ok(Event::default().data(line))).await.is_err() {
                        return; // browser disconnected
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, // dropped some — keep going
                Err(_) => return,                                                      // sender gone
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(out);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// Update the ring-buffer + recording config for a box (tenant admin only).
async fn patch_recording_config(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> Result<axum::response::Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "settings.write")?;
    let tid = tenant_of(&c)?;
    let updated = store::set_recording_config(&st.pool, tid, id, body.clone())
        .await
        .map_err(|e| {
            log::error!("set_recording_config: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;
    if !updated {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    Ok(axum::response::Json(body))
}

/// Update the object-tracking config (card: edge tracking) for a box — enabled/class/detect_ms/
/// lines. Mirrors [`patch_recording_config`]; the edge picks it up on its next restart (like
/// recording config — there's no live-reload poll for either yet).
async fn patch_tracking_config(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> Result<axum::response::Json<serde_json::Value>, ApiErr> {
    auth::require(&c, "settings.write")?;
    let tid = tenant_of(&c)?;
    let updated = store::set_tracking_config(&st.pool, tid, id, body.clone())
        .await
        .map_err(|e| {
            log::error!("set_tracking_config: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;
    if !updated {
        return Err(err(StatusCode::NOT_FOUND, "box not found"));
    }
    Ok(axum::response::Json(body))
}

/// Proxy GET /storage/stats from the box's edge appliance (JWT auth, recordings.read).
/// Returns { used_bytes, max_bytes, clip_count, path, pct } directly from the edge.
async fn storage_stats(
    UserClaims(c): UserClaims,
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl axum::response::IntoResponse, ApiErr> {
    use crate::domain::recordings::{normalize_host, edge_api_base};
    auth::require(&c, "recordings.read")?;
    let tid = tenant_of(&c)?;

    let edge_host = store::box_host_for_tenant(&st.pool, tid)
        .await
        .map_err(|e| {
            log::error!("box host lookup: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "no edge box connected"))?;

    let base = edge_api_base(&normalize_host(&edge_host));
    let url  = format!("{base}/storage/stats");

    let resp = reqwest::get(&url).await.map_err(|e| {
        log::error!("storage stats fetch {url}: {e}");
        err(StatusCode::BAD_GATEWAY, format!("edge unreachable: {e}"))
    })?;

    if !resp.status().is_success() {
        return Err(err(StatusCode::BAD_GATEWAY, format!("edge returned {}", resp.status())));
    }

    let json = resp.json::<serde_json::Value>().await.map_err(|e| {
        err(StatusCode::BAD_GATEWAY, format!("edge response parse error: {e}"))
    })?;

    Ok(axum::response::Json(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A code is read off one screen and typed into another. Everything a human plausibly does to
    /// it on the way — lowercase it, drop the dash, add a space, or confuse Crockford's lookalike
    /// characters — must still land on the same code, because the alternative is a customer staring
    /// at "that code isn't valid" while holding a valid code.
    #[test]
    fn normalize_forgives_how_humans_retype_a_code() {
        let canonical = normalize_code("ABCDE-FGHJK");
        for typed in ["abcde-fghjk", "ABCDEFGHJK", " abcde fghjk ", "abcde–fghjk", "ABCDE-fghjk"] {
            assert_eq!(normalize_code(typed), canonical, "{typed:?} should be the same code");
        }
    }

    /// Crockford's whole point: `O` is a zero, `I` and `L` are ones. Someone reading `0` off a
    /// screen and typing `O` has not made a mistake we should punish them for.
    #[test]
    fn normalize_maps_the_confusable_characters() {
        assert_eq!(normalize_code("O1IL"), "0111");
    }

    /// The alphabet must not *contain* the characters we fold away, or two distinct codes could
    /// normalize onto each other and one would silently unlock the other's box.
    #[test]
    fn the_alphabet_excludes_everything_normalize_rewrites() {
        for c in [b'I', b'L', b'O', b'U'] {
            assert!(!CODE_ALPHABET.contains(&c), "{} must not be mintable", c as char);
        }
    }

    #[test]
    fn a_minted_code_survives_its_own_normalization() {
        for _ in 0..200 {
            let code = new_pairing_code();
            assert_eq!(code.len(), 11, "ABCDE-FGHIJ");
            let norm = normalize_code(&code);
            assert_eq!(norm.len(), 10);
            // Minting then normalizing must be idempotent — otherwise the hash we store would never
            // match the hash we look up, and no code would ever redeem.
            assert_eq!(normalize_code(&norm), norm);
            assert!(norm.bytes().all(|b| CODE_ALPHABET.contains(&b)), "{code} left the alphabet");
        }
    }
}
