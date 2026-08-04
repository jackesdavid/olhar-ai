//! Control-plane REST: auth + RBAC + master console (see `handlers`), the edge
//! camera-config pull / discovery push, the edge installer + binaries, and health.
use axum::extract::{RawQuery, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use app_core::devices;
use app_core::onvif::DiscoveredDevice;
use app_core::tenancy::DEFAULT_TENANT_ID;

use crate::{handlers, store, AppState};

pub fn router(state: AppState) -> Router {
    // Edge appliance binaries (the repo is private, so they're served here, publicly,
    // instead of GitHub releases). CI ships them to EDGE_DIST_DIR on the VPS.
    let edge_dist = std::env::var("EDGE_DIST_DIR").unwrap_or_else(|_| "/edge-dist".to_string());

    // Everything the appliance calls. Split out so signature verification layers onto exactly
    // these routes and nothing else — the browser API and the public installer routes must not
    // pay for it, and `/enroll` cannot require a signature since it predates having a key.
    let edge_routes = Router::new()
        .route("/config/cameras", get(cameras))
        .route("/config/commands", get(commands))
        // Cloud→edge command transport (card #144): the edge holds this SSE stream and commands
        // ride down it; it POSTs each command's result back. Replaces the tailnet for commanding.
        .route("/config/edge/stream", get(crate::edge_stream::edge_stream))
        .route(
            "/config/edge/command-result",
            post(crate::edge_stream::command_result),
        )
        .route("/config/discovered", post(discovered_push))
        .route("/config/device-moved", post(device_moved))
        .route("/config/streaming", post(streaming_push))
        .route("/config/camera-status", post(camera_status_push))
        .route("/config/logs", post(box_logs_push))
        .route("/config/motion", post(motion_push))
        .route("/config/face", post(face_push))
        .route("/config/disk-alert", post(disk_alert_push))
        .route("/config/track-event", post(track_event_push))
        .route("/config/recording", get(recording_config))
        .route("/config/recordings/clip-metadata", post(clip_push))
        .route("/config/recordings/:clip_id/uploaded", post(clip_uploaded))
        .route("/config/tracking", get(tracking_config))
        // Bootstrap: bearer-authed, deliberately unsigned — it is how a box with no key gets one.
        .route("/config/signing-key", post(crate::box_signing::issue_key))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::box_signing::verify_signature,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/install.sh", get(install_script))
        .route("/edge-latest", get(edge_latest))
        .route("/edge/installers", get(installers))
        .route("/enroll", post(enroll))
        .merge(edge_routes)
        // Central token introspection — service-authenticated (see `service.rs`).
        .merge(crate::service::router())
        .merge(handlers::router())
        // Soft-lock: a lapsed/unpaid tenant is read-only (402 on writes).
        .merge(
            crate::domain::router().layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::billing_gate::soft_lock,
            )),
        )
        .nest_service("/edge", ServeDir::new(edge_dist))
        // Card #146: wrap every request in a tenant span so all logs during it ship to Datadog
        // tagged tenant:<id> — the central place, set once, not per handler.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            tenant_span,
        ))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Open a tracing span carrying the request's tenant (card #146). Best-effort: an absent/invalid
/// token just yields no tenant tag. Every log emitted during the request inherits `tenant:<id>`.
async fn tenant_span(
    State(st): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use tracing::Instrument;
    let tenant = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|tok| crate::auth::verify(&st.jwt_secret, tok.trim()).ok())
        .and_then(|c| c.tenant())
        .map(|t| t.to_string())
        .unwrap_or_default();
    next.run(req)
        .instrument(tracing::info_span!("http", tenant = %tenant))
        .await
}

/// Serve the edge appliance installer so a tenant admin's one-liner works.
async fn install_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        include_str!("../../sentinel-edge/install.sh"),
    )
}

/// One platform's published artifact, as recorded in `/edge-dist/manifest.json`.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PublishedArtifact {
    pub version: String,
    pub file: String,
    #[serde(default)]
    pub sha256: String,
}

/// Read the publication manifest: platform key -> artifact.
///
/// This is the ONLY source of "what is published". It used to come from `LATEST_EDGE_VERSION`,
/// which deploy.yml sed'd in from `sentinel-edge/Cargo.toml` on every deploy — i.e. from source, so
/// it advertised versions that may never have been built. macOS 404'd on those; Linux was worse and
/// silently re-downloaded a stale unversioned binary.
///
/// Missing or unparseable manifest returns empty rather than erroring: an appliance asking "what is
/// the latest?" should be told "nothing known", never handed a guess.
fn published() -> std::collections::BTreeMap<String, PublishedArtifact> {
    let dist = std::env::var("EDGE_DIST_DIR").unwrap_or_else(|_| "/edge-dist".to_string());
    let path = std::path::Path::new(&dist).join("manifest.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Default::default();
    };
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        platforms: std::collections::BTreeMap<String, PublishedArtifact>,
    }
    match serde_json::from_str::<Manifest>(&raw) {
        Ok(m) => m.platforms,
        Err(e) => {
            log::warn!("edge-dist manifest at {} is unreadable: {e}", path.display());
            Default::default()
        }
    }
}

/// The per-platform installer download set for the current edge version (card #84).
///
/// The web app renders whatever this returns, so a version bump is a control-only change — the
/// filenames are built from `LATEST_EDGE_VERSION` + the fixed naming scheme, never hardcoded in the
/// frontend. A file is only listed if it **actually exists** in `EDGE_DIST_DIR`: a download button
/// that 404s is worse than an absent one, and this also gracefully covers "Windows isn't published
/// for this version yet." Linux has no single file — it's the `/install.sh` one-liner, always shown.
async fn installers() -> impl IntoResponse {
    let dist = std::env::var("EDGE_DIST_DIR").unwrap_or_else(|_| "/edge-dist".to_string());
    let pubs = published();

    // Filenames come from the manifest, not from a guessed naming scheme — each platform carries
    // its OWN version now, because macOS is built out-of-band and legitimately lags Linux.
    let files: Vec<(&str, &str, &str, String)> = [
        ("macos", "macOS (Apple Silicon + Intel)", "pkg"),
        ("windows", "Windows 10/11 (64-bit)", "zip"),
    ]
    .iter()
    .filter_map(|(os, label, kind)| {
        let a = pubs.get(*os)?;
        Some((*os, *label, *kind, a.file.clone()))
    })
    .collect();

    let mut installers: Vec<serde_json::Value> = files
        .iter()
        .filter_map(|(os, label, kind, filename)| {
            // Only offer what's on disk — the button must never 404.
            let size = std::fs::metadata(std::path::Path::new(&dist).join(filename))
                .ok()?
                .len();
            Some(json!({
                "os": os,
                "label": label,
                "kind": kind,
                "url": format!("/edge/{filename}"),
                "size_bytes": size,
                // Per-platform: macOS is built out-of-band, so it legitimately differs from Linux.
                "version": pubs.get(*os).map(|a| a.version.clone()),
            }))
        })
        .collect();

    // Linux is always available as the one-liner — no single artifact to stat.
    installers.push(json!({
        "os": "linux",
        "label": "Linux (one-line install)",
        "kind": "sh",
        "url": "/install.sh",
        "size_bytes": serde_json::Value::Null,
    }));

    // No single "version" any more — each installer carries its own, since platforms now
    // publish independently. Kept as a map so the app can show per-platform versions.
    Json(json!({ "installers": installers }))
}

// ---- enrollment by pairing code (card #81) ----------------------------------
//
// This endpoint is **unauthenticated by necessity**: the box redeeming a code has no credential yet
// — acquiring one is the whole point. What protects it:
//
//   * the code's entropy (10 symbols of Crockford base32 ≈ 1.1e15) inside a 15-minute window,
//   * single use, claimed atomically in one UPDATE,
//   * the rate limiter below,
//   * and one generic error for every failure — wrong, expired, already spent. A caller learns
//     nothing from the response, so it can't be used as an oracle to sift valid codes.
//
// Nothing secret is stored to make this work: redeeming MINTS a fresh box token and a fresh
// Tailscale key here, on the spot.

/// Failed redeems allowed per client, per window. Generous for a human fat-fingering a code off a
/// screen; nowhere near enough to search a 1.1e15 space.
const ENROLL_MAX_FAILURES: u32 = 10;
const ENROLL_WINDOW_SECS: u64 = 300;

/// Per-client failure counters. In memory on purpose: one control pod, no moving parts, and it
/// resets on deploy. Putting it in Postgres would mean an unauthenticated request writes a row —
/// which is a DoS lever, not a defence.
fn enroll_limiter() -> &'static std::sync::Mutex<HashMap<String, (u32, std::time::Instant)>> {
    static LIMITER: std::sync::OnceLock<
        std::sync::Mutex<HashMap<String, (u32, std::time::Instant)>>,
    > = std::sync::OnceLock::new();
    LIMITER.get_or_init(Default::default)
}

/// Who is asking. Control sits behind the VPS reverse proxy, so the peer address is the proxy.
///
/// **The LAST `X-Forwarded-For` hop, not the first.** The first hop is whatever the client typed:
/// a request arriving with `X-Forwarded-For: 1.2.3.4` gets that value appended to, so trusting the
/// front of the list lets anyone pick their own rate-limit bucket and defeat the limiter with a
/// counter of their own. The last hop is the one our proxy appended, which is the only entry the
/// client could not choose. `X-Real-Ip` is preferred where the proxy sets it, since it is
/// single-valued and unambiguous.
///
/// An absent header collapses everyone into one shared bucket, which fails *closed* (stricter).
fn enroll_client(headers: &HeaderMap) -> String {
    let real_ip = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(ip) = real_ip {
        return ip.to_string();
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn enroll_rate_limited(client: &str) -> bool {
    let mut map = enroll_limiter().lock().unwrap();
    let window = std::time::Duration::from_secs(ENROLL_WINDOW_SECS);
    map.retain(|_, (_, at)| at.elapsed() < window);
    map.get(client)
        .map(|(n, _)| *n >= ENROLL_MAX_FAILURES)
        .unwrap_or(false)
}

fn enroll_record_failure(client: &str) {
    let mut map = enroll_limiter().lock().unwrap();
    let e = map
        .entry(client.to_string())
        .or_insert((0, std::time::Instant::now()));
    e.0 += 1;
    e.1 = std::time::Instant::now();
}

#[derive(serde::Deserialize)]
struct EnrollReq {
    code: String,
}

/// Redeem a pairing code → everything the edge needs to become this tenant's box.
async fn enroll(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EnrollReq>,
) -> Response {
    // One message for every failure mode. Deliberately uninformative.
    let refuse = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "that code isn't valid — check it and try again, or generate a new one" })),
        )
            .into_response()
    };

    let client = enroll_client(&headers);
    if enroll_rate_limited(&client) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "too many attempts — wait a few minutes and try again" })),
        )
            .into_response();
    }

    let code = crate::domain::boxes::normalize_code(&body.code);
    if code.is_empty() {
        enroll_record_failure(&client);
        return refuse();
    }

    let hash = crate::domain::boxes::hash_token(&code);
    let claimed = match crate::store::claim_pairing(&st.pool, &hash).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            enroll_record_failure(&client);
            return refuse();
        }
        Err(e) => {
            log::error!("enroll: claim failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response();
        }
    };
    let (_tenant_id, box_id) = claimed;

    // The code is spent. Mint the box's real credentials now — this is why none of them had to be
    // stored anywhere.
    let token = crate::domain::boxes::new_box_token();
    if let Err(e) =
        crate::store::rotate_box_token(&st.pool, box_id, &crate::domain::boxes::hash_token(&token))
            .await
    {
        log::error!("enroll: token rotate failed for box {box_id}: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "could not issue a token").into_response();
    }
    // The bearer just changed hands; any signing key issued to the previous holder must not keep
    // verifying. The re-enrolling appliance bootstraps a fresh one on its next boot.
    crate::box_signing::revoke_keys(&st.pool, box_id).await;

    let name = crate::store::box_name(&st.pool, box_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let hostname = format!("sentinel-{}", &box_id.to_string()[..8]);
    // No tailnet (card #144) — enrollment issues only the box token; the edge is outbound-only.
    log::info!("box {box_id} ({name}) enrolled by pairing code");
    Json(json!({
        "box_id":    box_id,
        "box_token": token,
        "hostname":  hostname,
    }))
    .into_response()
}

/// Map what a box REPORTS about itself onto a manifest key.
///
/// The two vocabularies were never the same and nothing bridged them: a box reports
/// `"macOS (x86_64)"` or `"Ubuntu (x86_64)"` (built for display), while the manifest is keyed
/// `macos` / `linux-x86_64` / `linux-aarch64`. Callers holding a box row could therefore not ask
/// about that box, and fell back to the no-platform answer — the lowest version across all
/// platforms — which is how a macOS box on 0.5.66 got told the latest was Linux's 0.5.65.
///
/// Accepts a manifest key unchanged, so `?platform=macos` keeps working.
fn platform_key(reported: &str) -> Option<&'static str> {
    let s = reported.to_ascii_lowercase();
    if s.contains("mac") || s.contains("darwin") || s.contains("os x") {
        return Some("macos");
    }
    if s.contains("windows") || s.contains("win32") {
        return Some("windows");
    }
    // Everything else that names an arch we publish for is treated as Linux — the distro varies
    // (Ubuntu, Fedora, Bazzite, JetPack) but the binary is per-arch, not per-distro.
    let arm = s.contains("aarch64") || s.contains("arm64");
    let x86 = s.contains("x86_64") || s.contains("amd64");
    if arm {
        return Some("linux-aarch64");
    }
    if x86 {
        return Some("linux-x86_64");
    }
    None
}

/// The latest edge build the operator has published (for the "update available" badge).
async fn edge_latest(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let pubs = published();

    // `?platform=` is the correct call: each platform has its own published version. The value may
    // be a manifest key or the string the box reports about itself — both resolve here, so a caller
    // holding a box row can pass `box.platform` straight through.
    if let Some(raw) = q.get("platform") {
        let key = if pubs.contains_key(raw.as_str()) {
            raw.as_str()
        } else {
            platform_key(raw).unwrap_or(raw.as_str())
        };
        return match pubs.get(key) {
            Some(a) => Json(json!({
                "version": a.version, "file": a.file, "sha256": a.sha256
            })),
            // Nothing published for this platform — say so plainly. An appliance that is told
            // "no update" stays on a working build; one told about a version that was never
            // built tries to fetch it and fails, which is the bug this replaces.
            None => Json(json!({ "version": null })),
        };
    }

    // No platform given: an appliance built before this endpoint took the parameter. Such a box
    // uses this answer for ONE decision — "is there anything newer than me?" — and then hands the
    // real work to `install.sh`, which resolves the correct per-platform build and refuses to move
    // backwards. So this value is a TRIGGER TO GO LOOK, not an install target.
    //
    // It used to answer with the LOWEST version across platforms, meaning to be conservative. That
    // was actively harmful: a box on the newest macOS build was told the oldest Linux one, matched
    // its own version, and concluded it was current — so it never fetched install.sh, and the fix
    // for its own updater could never arrive. The conservatism protected against pointing a box at
    // a build its platform lacks, which install.sh now handles properly at the only layer that can
    // actually see the platform.
    //
    // MAX is therefore the correct trigger: if anything anywhere is newer, go and find out. A box
    // already current for its platform learns that from install.sh and does nothing.
    let max = pubs.values().map(|a| a.version.as_str()).max();
    Json(json!({ "version": max }))
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let n = devices::repo::load(&st.db, DEFAULT_TENANT_ID).len();
    Json(json!({ "status": "ok", "service": "sentinel-control", "cameras": n }))
}

/// Resolve a box token (Authorization: Bearer) → its tenant, stamping last_seen, the
/// reported tailnet host (`X-Box-Host`), and the edge version (`X-Edge-Version`).
/// `Ok(None)` = no token presented; `Err(resp)` = a token was presented but invalid.
async fn box_tenant(st: &AppState, headers: &HeaderMap) -> Result<Option<uuid::Uuid>, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else { return Ok(None) };

    let host = headers.get("x-box-host").and_then(|v| v.to_str().ok());
    let version = headers.get("x-edge-version").and_then(|v| v.to_str().ok());
    let (ts, gs, plat, hw) = box_health_headers(headers);
    let metrics = box_metrics_header(headers);
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::touch_box_by_token(
        &st.pool, &hash, host, version, ts, gs, plat, hw, metrics,
    )
    .await
    {
        Ok(Some(tid)) => Ok(Some(tid)),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "unknown box token").into_response()),
        Err(e) => {
            log::error!("box token lookup failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response())
        }
    }
}

/// Extract the edge health headers (`X-Edge-Tailscale`, `X-Edge-Gstreamer`, `X-Edge-Platform`,
/// `X-Edge-Hwaccel`) — present only on the box's regular config poll, absent on the high-rate
/// pushes (then `None` keeps the prior value via COALESCE).
fn box_health_headers(
    headers: &HeaderMap,
) -> (
    Option<&str>,
    Option<bool>,
    Option<&str>,
    Option<&str>,
) {
    let ts = headers
        .get("x-edge-tailscale")
        .and_then(|v| v.to_str().ok());
    let gs = headers
        .get("x-edge-gstreamer")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == "1");
    let plat = headers.get("x-edge-platform").and_then(|v| v.to_str().ok());
    let hw = headers.get("x-edge-hwaccel").and_then(|v| v.to_str().ok());
    (ts, gs, plat, hw)
}

/// Parse the `X-Edge-Metrics` heartbeat header (compact JSON: cpu/mem/disk/gpu percents).
/// Absent on the high-rate pushes → `None` keeps the prior snapshot via COALESCE.
fn box_metrics_header(headers: &HeaderMap) -> Option<app_core::edge_metrics::EdgeMetrics> {
    headers
        .get("x-edge-metrics")
        .and_then(|v| v.to_str().ok())
        .and_then(app_core::edge_metrics::EdgeMetrics::from_header)
}

/// Whether a token-less `/config/cameras` may fall back to the default tenant. **Only ever true in
/// a debug build.** The `cfg!` is the point: it makes the escape hatch a property of the binary, not
/// of the environment it's deployed with — a release binary cannot be talked into serving one
/// tenant's cameras (and their RTSP credentials) to an unauthenticated caller, no matter what env
/// somebody sets on the VPS.
fn untenanted_dev_config() -> bool {
    cfg!(debug_assertions)
        && std::env::var("ALLOW_UNTENANTED_CONFIG")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
            .unwrap_or(false)
}

/// The camera list an edge box pulls, scoped to the box's tenant.
///
/// **A box token is required, and in a release build there is no way around that.** This response
/// carries RTSP credentials, control is a public endpoint, and the caller here is by definition
/// unauthenticated-until-proven — so a no-token request must never be answered with *somebody's*
/// cameras. The `ALLOW_UNTENANTED_CONFIG` escape hatch (a single-tenant dev edge pointed at the
/// default tenant) is compiled out of release builds entirely: `debug_assertions` is off there, so
/// the env var is not even read. Setting it on the VPS by accident does nothing.
async fn cameras(
    State(st): State<AppState>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(tid)) => tid,
        Ok(None) if untenanted_dev_config() => DEFAULT_TENANT_ID,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };

    // Long-poll: if `?wait=<secs>` and nothing is already staged, hold the request until
    // the web changes a camera (or the timeout), so edge edits apply in ~1s, not on a
    // timer (and idle edges don't re-pull/re-resolve).
    let wait: u64 = q
        .unwrap_or_default()
        .split('&')
        .find_map(|p| p.strip_prefix("wait="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .min(60);
    let has_pending = st
        .pending_creds
        .lock()
        .unwrap()
        .keys()
        .any(|(t, _)| *t == tenant);
    if wait > 0 && !has_pending {
        let n = st.notifier(tenant);
        let _ = tokio::time::timeout(Duration::from_secs(wait), n.notified()).await;
    }

    let mut list = devices::repo::load(&st.db, tenant);
    // Hand over any credentials the web staged for this tenant's cameras — ONCE — then
    // discard them. Camera credentials are never persisted in the control DB.
    {
        let mut pend = st.pending_creds.lock().unwrap();
        pend.retain(|_, (_, _, t)| t.elapsed() < Duration::from_secs(1800)); // drop stale
        for d in list.iter_mut() {
            if let Some((u, p, _)) = pend.remove(&(tenant, d.ip.clone())) {
                d.username = Some(u);
                d.password = Some(p);
            }
        }
    }
    Json(list).into_response()
}

/// Like [`box_tenant`] but also resolves the box id (for log tagging).
pub(crate) async fn box_identity(
    st: &AppState,
    headers: &HeaderMap,
) -> Result<Option<(uuid::Uuid, uuid::Uuid)>, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else { return Ok(None) };
    let host = headers.get("x-box-host").and_then(|v| v.to_str().ok());
    let version = headers.get("x-edge-version").and_then(|v| v.to_str().ok());
    let (ts, gs, plat, hw) = box_health_headers(headers);
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::touch_box_identity(&st.pool, &hash, host, version, ts, gs, plat, hw).await {
        Ok(Some(pair)) => Ok(Some(pair)),
        Ok(None) => Err((StatusCode::UNAUTHORIZED, "unknown box token").into_response()),
        Err(e) => {
            log::error!("box token lookup failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "lookup failed").into_response())
        }
    }
}

/// The edge polls this each cycle (box token) for pending commands; the rescan flag is
/// consumed here so the next poll won't re-trigger it.
async fn commands(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let (tenant, box_id) = match box_identity(&st, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    // rescan is tenant-wide (Devices "Rescan"); restart/update are per-box and DB-backed.
    let (restart, update) = st.take_box_commands(box_id).await;
    Json(json!({
        "rescan":  st.take_rescan(tenant),
        "restart": restart,
        "update":  update,
    }))
    .into_response()
}

/// A camera that changed IP, reported by the edge (matched on its stable ONVIF id).
#[derive(serde::Deserialize)]
struct DeviceMove {
    onvif_id: String,
    #[serde(default)]
    old_ip: String,
    new_ip: String,
    #[serde(default)]
    xaddr: String,
}

/// The edge reports cameras whose IP changed so the cloud device rows + Live page follow
/// the move (box token required). Keyed by stable ONVIF id.
async fn device_moved(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<Vec<DeviceMove>>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let moves = body.map(|Json(m)| m).unwrap_or_default();
    if moves.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let db = st.db.clone();
    let n = moves.len();
    let _ = tokio::task::spawn_blocking(move || {
        for m in &moves {
            devices::repo::move_ip(&db, tenant, &m.onvif_id, &m.old_ip, &m.new_ip, &m.xaddr);
        }
    })
    .await;
    log::info!("box reconciled {n} camera IP change(s) for tenant {tenant}");
    st.bump(tenant); // wake the edge so it re-pulls the corrected list
    StatusCode::NO_CONTENT.into_response()
}

/// One camera in the edge's streaming report — resolved-stream IP + probed codec, if the edge
/// knows it (gstreamer-linked backend only, and only once probed). `codec` is `None` on an older
/// edge build's plain-string report too, via [`StreamingIn`]'s untagged fallback.
#[derive(serde::Deserialize)]
struct StreamingCamIn {
    ip: String,
    #[serde(default)]
    codec: Option<String>,
}

/// Accepts either the old wire shape (`["1.2.3.4", ...]`) or the current one
/// (`[{"ip": "1.2.3.4", "codec": "H264"}, ...]`) so control never breaks for an edge that hasn't
/// auto-updated yet.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StreamingIn {
    Rich(Vec<StreamingCamIn>),
    Plain(Vec<String>),
}

impl StreamingIn {
    fn into_cams(self) -> Vec<StreamingCamIn> {
        match self {
            Self::Rich(v) => v,
            Self::Plain(ips) => ips
                .into_iter()
                .map(|ip| StreamingCamIn { ip, codec: None })
                .collect(),
        }
    }
}

/// How long a camera was out, from the last frame it delivered to now. `None` when it has never
/// delivered one (a camera that has been broken since it was added was never "online" to time).
fn offline_seconds(last_frame_at: Option<&str>, now: &str) -> Option<i64> {
    app_core::timeutil::secs_between(last_frame_at?, now)
}

/// "4m" / "2h" — for event copy only; the machine-readable value is `offline_secs`.
fn human_dur(secs: i64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m", secs / 60);
    }
    if secs < 86_400 {
        return format!("{}h", secs / 3600);
    }
    format!("{}d", secs / 86_400)
}

/// The edge reports which cameras it currently has a resolved stream for (box token). Drives
/// the precise "online & correct" status dot + codec badge on the Devices page.
/// `POST /config/camera-status` — the edge reports each camera's stage, and control turns HEALTH
/// CHANGES into `camera_offline` / `camera_online` events (card #155).
///
/// **Transition-only.** A camera that is down for a day emits ONE event, not one per report: the
/// previous status is read before the write and compared. A first-ever report is not a transition
/// and fires nothing — otherwise adding a camera would announce it as "came online", and a control
/// restart would replay every camera in the tenant.
async fn camera_status_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<Vec<store::CameraStatusIn>>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let items = body.map(|Json(v)| v).unwrap_or_default();
    let now = app_core::timeutil::now_rfc3339();

    for s in &items {
        let healthy_now = matches!(s.status.as_str(), "ok" | "streaming");
        // Read BEFORE the write — RETURNING would hand back the new value (see the store helper).
        let was = match store::previous_camera_state(&st.pool, tenant, &s.ip).await {
            Ok(v) => v,
            Err(e) => {
                log::error!("camera-status: previous health for {}: {e}", s.ip);
                continue;
            }
        };
        if let Err(e) = store::record_camera_status(&st.pool, tenant, s, &now).await {
            log::error!("camera-status: write for {}: {e}", s.ip);
            continue;
        }

        // `None` = never probed before. Not a transition.
        let Some((was_healthy, was_last_frame)) = was else {
            continue;
        };
        if was_healthy == healthy_now {
            continue;
        }

        let name = store::device_name(&st.pool, tenant, &s.ip)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| s.ip.clone());

        let (kind, title, message, data) = if healthy_now {
            // How long it was actually out, measured from the last frame that arrived. `null` when
            // the camera has never delivered one — an honest gap beats a confident `0`, which would
            // make "notify only if it was down over 5 minutes" silently never fire.
            let offline_secs = offline_seconds(was_last_frame.as_deref(), &now);
            let human = match offline_secs {
                Some(secs) => format!("{name} is responding again (out for {})", human_dur(secs)),
                None => format!("{name} is responding again"),
            };
            (
                "camera_online",
                "Camera back online",
                human,
                serde_json::json!({ "camera": s.ip, "offline_secs": offline_secs }),
            )
        } else {
            (
                "camera_offline",
                "Camera offline",
                format!("{name}: {}", s.detail),
                serde_json::json!({ "camera": s.ip, "reason": s.status, "detail": s.detail }),
            )
        };

        let mut ev = app_core::events::NewEvent::new(tenant, kind, "system");
        ev.severity = if healthy_now { "info" } else { "warn" }.to_string();
        ev.title = Some(title.to_string());
        ev.message = Some(message);
        ev.camera = Some(s.ip.clone());
        ev.data = Some(data);
        st.event_queue.push(ev);
        log::info!("camera {} → {} ({})", s.ip, s.status, kind);
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn streaming_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<StreamingIn>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let cams = body.map(|Json(v)| v.into_cams()).unwrap_or_default();
    let rows: Vec<(String, Option<String>)> = cams.into_iter().map(|c| (c.ip, c.codec)).collect();
    if let Err(e) = store::upsert_streaming(&st.pool, tenant, &rows).await {
        log::error!("upsert_streaming failed: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "store failed").into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// One motion observation reported by the edge.
#[derive(serde::Deserialize)]
struct MotionIn {
    camera_ip: String,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    area: i64,
    #[serde(default)]
    blobs: i64,
    #[serde(default)]
    bbox: [i32; 4],
    #[allow(dead_code)]
    #[serde(default)]
    at: String,
    /// The JPEG frame that triggered detection (base64). Relayed to flow so `OnEvent("motion")`
    /// can expose it as an Image output without a second round-trip to the edge.
    #[serde(default)]
    frame_b64: Option<String>,
}

/// The edge reports a motion observation (box token → tenant). We publish it onto the Redis
/// fabric (`flow:inject`) so flow fires a `motion` System event to that tenant's `OnEvent`
/// handler graphs, and (once per tenant per process) ensure the `motion` event is registered
/// with its typed payload so OnEvent nodes expose camera/score/area/blobs/bbox outputs.
async fn motion_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<MotionIn>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let Some(Json(ev)) = body else {
        return (StatusCode::BAD_REQUEST, "motion body required").into_response();
    };

    // bbox is a Bbox port (canonical xyxy). The edge reports xywh, so convert here:
    // [x, y, w, h] → [x1, y1, x2, y2]. Sent as a JSON array; flow seeds it into a
    // PortValue::Bbox for the OnEvent("motion").bbox output.
    let [x, y, w, h] = ev.bbox;
    let bbox_xyxy = [x, y, x + w, y + h];
    let mut payload = serde_json::json!({
        "camera": ev.camera_ip,
        "score": ev.score.round() as i64,
        "area": ev.area,
        "blobs": ev.blobs,
        "bbox": bbox_xyxy,
    });
    if let (Some(obj), Some(f)) = (payload.as_object_mut(), ev.frame_b64) {
        obj.insert("frame".to_string(), serde_json::Value::String(f));
    }
    // Durable inbound trigger: enqueue into the Postgres event_queue (card #57). Flow's
    // event_consumer claims it and fires the tenant's OnEvent("motion") handlers — it survives
    // a flow restart and is never dropped (unlike the old Redis flow:inject, which vanished
    // whenever REDIS_URL was unset).
    app_core::work_queue::enqueue_event_blocking(&st.db, tenant, "motion", "edge", &payload);
    StatusCode::NO_CONTENT.into_response()
}

/// One face detection reported by the edge (YuNet). `bbox` is canonical xyxy source pixels.
#[derive(serde::Deserialize)]
struct FaceIn {
    camera_ip: String,
    #[serde(default)]
    bbox: [i32; 4], // xyxy
    #[serde(default)]
    confidence: f64, // 0..100
    #[serde(default)]
    face_width: i64, // face box width in source pixels (face-size signal)
    #[serde(default)]
    kps: Vec<i32>, // YuNet 5 landmarks, flat [x0,y0,…] source px (#122)
    // Quality signals (#126). `Option` rather than `#[serde(default)]`, because for these the
    // difference between "reported as zero" and "not reported at all" decides whether a gate
    // should fire — an edge older than #126 sends neither, and 0 is a real, meaningful blur value.
    #[serde(default)]
    blur: Option<f64>,
    #[serde(default)]
    yaw: Option<f64>,
    #[serde(default)]
    pitch: Option<f64>,
    #[serde(default)]
    roll: Option<f64>,
    #[allow(dead_code)]
    #[serde(default)]
    at: String,
    #[serde(default)]
    frame_b64: Option<String>,
}

/// The edge reports a face detection (box token → tenant). Durable-enqueued as a `face_detected`
/// System event so the tenant's `OnEvent("face_detected")` graphs fire with the frame + bbox +
/// confidence + face_width — the entry point of the face recognition pipeline (#117).
async fn face_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<FaceIn>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let Some(Json(ev)) = body else {
        return (StatusCode::BAD_REQUEST, "face body required").into_response();
    };
    // Quality ports (#126). A box that predates them reports nothing, and a gate built on a
    // missing `blur` would fail CLOSED (`run_num_cmp` resolves an absent input to 0), silently
    // killing that tenant's whole face pipeline until every box updated. So an unreported
    // sharpness becomes an explicit fail-open sentinel; pose defaults to 0, which already passes
    // the `NumLte(|angle|)` direction a pose gate is written in.
    let blur = ev.blur.unwrap_or(app_core::agent::face_quality::BLUR_UNKNOWN);
    let mut payload = serde_json::json!({
        "camera": ev.camera_ip,
        "bbox": ev.bbox, // already xyxy
        "confidence": ev.confidence.round() as i64,
        "face_width": ev.face_width,
        "kps": ev.kps, // flat [x0,y0,…] → face_detected Landmarks port → ArcFace align (#122)
        "blur": blur.round() as i64,
        "yaw": ev.yaw.unwrap_or(0.0).round() as i64,
        "pitch": ev.pitch.unwrap_or(0.0).round() as i64,
        "roll": ev.roll.unwrap_or(0.0).round() as i64,
    });
    if let (Some(obj), Some(f)) = (payload.as_object_mut(), ev.frame_b64) {
        obj.insert("frame".to_string(), serde_json::Value::String(f));
    }
    app_core::work_queue::enqueue_event_blocking(&st.db, tenant, "face_detected", "edge", &payload);
    StatusCode::NO_CONTENT.into_response()
}

/// The edge reports disk usage is near/over the configured limit (box token → tenant). Enqueues
/// an `edge:full-disk` event into the durable `event_queue` so tenant `OnEvent("edge:full-disk")`
/// workflow graphs can react (send alert, stop recordings, etc.).
async fn disk_alert_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<serde_json::Value>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let payload = body.map(|Json(v)| v).unwrap_or_default();
    app_core::work_queue::enqueue_event_blocking(
        &st.db,
        tenant,
        "edge:full-disk",
        "edge",
        &payload,
    );
    StatusCode::NO_CONTENT.into_response()
}

/// An object-tracker lifecycle event from the edge (card: edge tracking). `kind` is the event name
/// (`object_entered` / `object_exited` / `line_crossed`); the rest is the typed payload an
/// `OnEvent` node reads. Bbox arrives canonical xyxy (the edge already works in source pixels;
/// meaningless — `[0;4]` — for `line_crossed` (carries `line`/`direction` instead) and for a
/// zone-scoped `object_entered`/`object_exited` (carries `zone` instead). Durable-enqueued like
/// motion, so it survives a flow restart.
#[derive(serde::Deserialize)]
struct TrackEventIn {
    kind: String,
    camera_ip: String,
    uuid: String,
    class: String,
    bbox: [i32; 4],
    count: i64,
    #[serde(default)]
    dwell_secs: Option<i64>,
    #[serde(default)]
    line: Option<String>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    zone: Option<String>,
}

async fn track_event_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<TrackEventIn>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let Some(Json(ev)) = body else {
        return (StatusCode::BAD_REQUEST, "track-event body required").into_response();
    };
    // Only the tracking events are accepted (guards against a stray/mistyped kind enqueuing junk).
    if ev.kind != "object_entered" && ev.kind != "object_exited" && ev.kind != "line_crossed" {
        return (StatusCode::BAD_REQUEST, "unknown track-event kind").into_response();
    }
    let mut payload = json!({
        "uuid": ev.uuid,
        "class": ev.class,
        "camera": ev.camera_ip,
        "bbox": ev.bbox,
        "count": ev.count,
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(d) = ev.dwell_secs {
            obj.insert("dwell_secs".to_string(), json!(d));
        }
        if let (Some(line), Some(direction)) = (ev.line, ev.direction) {
            obj.insert("line".to_string(), json!(line));
            obj.insert("direction".to_string(), json!(direction));
        }
        if let Some(zone) = ev.zone {
            obj.insert("zone".to_string(), json!(zone));
        }
    }
    app_core::work_queue::enqueue_event_blocking(&st.db, tenant, &ev.kind, "edge", &payload);
    StatusCode::NO_CONTENT.into_response()
}

/// Newest box-log rows kept per box (bounded retention; the edge keeps its own ring + stdout).

/// The edge pushes a batch of its recent log lines here (box token). Control persists a
/// bounded tail and re-broadcasts each line for any live per-box log viewer.
async fn box_logs_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<Vec<store::LogLineIn>>>,
) -> Response {
    let (tenant, box_id) = match box_identity(&st, &headers).await {
        Ok(Some(pair)) => pair,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let lines = body.map(|Json(l)| l).unwrap_or_default();
    if lines.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    // Card #146: the box holds its token, not its name — control resolves `edge_name` here (once per
    // batch) so every edge log in Datadog can be faceted by a human name, not just the box id.
    let edge_id = box_id.to_string();
    let edge_name = store::box_name(&st.pool, box_id)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    // Card #145/#146: the edge keeps relaying its lines here; they go to the unified log queue, which
    // ships to Datadog when configured (else Postgres). Structured fields (`camera_ip`, `fps`, …) plus
    // the stamped identity ride in `payload` and are spread to searchable attributes / facets by the
    // Datadog record builder. No more `box_logs` table or `log_bus` broadcast (in-app view is gone).
    for l in &lines {
        let mut payload = l.fields.clone();
        payload.insert("target".into(), json!(l.target));
        payload.insert("ts".into(), json!(l.ts));
        payload.insert("edge_id".into(), json!(edge_id));
        if let Some(name) = &edge_name {
            payload.insert("edge_name".into(), json!(name));
        }
        st.log_queue.push(app_core::platform_logs::PlatformLog {
            tenant_id: tenant,
            module: "edge",
            trace_id: None,
            entity_id: Some(edge_id.clone()),
            entity_type: Some("edge_box"),
            level: match l.level.to_lowercase().as_str() {
                "error" => "error",
                "warn" => "warn",
                "debug" => "debug",
                _ => "info",
            },
            message: l.message.clone(),
            payload: Some(serde_json::Value::Object(payload)),
        });
    }
    StatusCode::NO_CONTENT.into_response()
}

/// The edge pushes its latest ONVIF discovery sweep here (box token required); control
/// stores it tenant-scoped for the cloud Devices page.
async fn discovered_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<Vec<DiscoveredDevice>>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(tid)) => tid,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let cams = body.map(|Json(c)| c).unwrap_or_default();
    match store::upsert_discovered(&st.pool, tenant, &cams).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            log::error!("upsert_discovered failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "store failed").into_response()
        }
    }
}

/// Return the recording config for the authenticated edge box.
async fn recording_config(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else {
        return (StatusCode::UNAUTHORIZED, "box token required").into_response();
    };
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::recording_config_by_token(&st.pool, &hash).await {
        Ok(Some(cfg)) => Json(cfg).into_response(),
        Ok(None) => (StatusCode::UNAUTHORIZED, "unknown box token").into_response(),
        Err(e) => {
            log::error!("recording_config_by_token: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

/// Return the tracking config (card: edge tracking) for the authenticated edge box — the
/// per-tenant enabled/class/detect_ms/lines the edge prefers over its env-var defaults.
/// Mirrors [`recording_config`].
async fn tracking_config(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(tok) = token else {
        return (StatusCode::UNAUTHORIZED, "box token required").into_response();
    };
    let hash = hex::encode(Sha256::digest(tok.as_bytes()));
    match store::tracking_config_by_token(&st.pool, &hash).await {
        Ok(Some(cfg)) => Json(cfg).into_response(),
        Ok(None) => (StatusCode::UNAUTHORIZED, "unknown box token").into_response(),
        Err(e) => {
            log::error!("tracking_config_by_token: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

/// The edge pushes a finalised clip manifest (box token). Persists metadata
/// and fans out a `recording-ready` SSE event to the tenant's browsers.
async fn clip_push(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<crate::domain::recordings::ClipPush>>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    let Some(Json(clip)) = body else {
        return (StatusCode::BAD_REQUEST, "clip body required").into_response();
    };
    let clip_id = clip.clip_id.clone();
    let started_at = clip.started_at.clone();
    let camera_ip = clip.camera_ip.clone();
    let duration_secs = clip.duration_secs;
    let id = match crate::domain::recordings::push_clip(&st.pool, tenant, clip).await {
        Ok(id) => id,
        Err(e) => {
            log::error!("clip push failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
        }
    };

    // The recording is the timeline's scrub anchor (card #66, Phase 3): pin the event to when the
    // clip began, carrying the clip id + camera + span so a future Live view can jump to it.
    let mut ev = app_core::events::NewEvent::new(tenant, "clip.started", "edge");
    ev.occurred_at = started_at.clone();
    ev.camera = Some(camera_ip.clone());
    ev.clip_id = Some(clip_id.clone());
    ev.message = Some(format!("Recording on {camera_ip} ({duration_secs}s)"));
    ev.data = Some(serde_json::json!({ "duration_secs": duration_secs, "recording_id": id }));
    st.event_queue.push(ev);

    // Offer the edge a short-lived URL to upload the clip straight to R2 — no video crosses
    // this VPS, and the edge never holds our credentials. Best-effort: a clip that isn't
    // offered an upload is still recorded, it just stays local.
    let upload = upload_ticket(&st, tenant, id, &clip_id, &started_at).await;
    match upload {
        Some((url, key)) => {
            Json(serde_json::json!({ "id": id, "upload": { "url": url, "key": key } }))
                .into_response()
        }
        None => Json(serde_json::json!({ "id": id })).into_response(),
    }
}

/// How long the edge has to finish the upload before the URL expires.
const UPLOAD_URL_TTL_SECS: u32 = 3600;

/// `Some((presigned_put_url, object_key))` when the tenant's plan sells cloud recording and the
/// bucket's retention rules are proven present.
async fn upload_ticket(
    st: &AppState,
    tenant: uuid::Uuid,
    id: i64,
    clip_id: &str,
    started_at: &str,
) -> Option<(String, String)> {
    if !app_core::clip_store::armed() {
        return None;
    }
    let days = crate::store::tenant_retention_days(&st.pool, tenant)
        .await
        .unwrap_or(0);
    if days <= 0 {
        return None;
    }
    let key = app_core::clip_store::object_key(days, tenant, started_at, clip_id);

    // `presign_put` drives its own runtime; calling it on this async task would panic.
    let k = key.clone();
    let url = tokio::task::spawn_blocking(move || {
        app_core::clip_store::presign_put(&k, UPLOAD_URL_TTL_SECS)
    })
    .await
    .ok()?
    .map_err(|e| log::error!("clip upload presign: {e}"))
    .ok()?;

    if let Err(e) = crate::store::set_clip_uploading(&st.pool, id, &key).await {
        log::error!("clip {clip_id}: could not mark uploading: {e}");
        return None;
    }
    Some((url, key))
}

/// The edge confirms the object landed in the bucket. Until this arrives the clip stays
/// `uploading` and the UI shows it as in transit, never as durable.
async fn clip_uploaded(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(clip_id): axum::extract::Path<String>,
) -> Response {
    let tenant = match box_tenant(&st, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "box token required").into_response(),
        Err(resp) => return resp,
    };
    match crate::store::mark_clip_uploaded(&st.pool, tenant, &clip_id).await {
        Ok(0) => (StatusCode::NOT_FOUND, "no such clip awaiting upload").into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            log::error!("clip {clip_id} confirm failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "confirm failed").into_response()
        }
    }
}

#[cfg(test)]
mod enroll_client_tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        m
    }

    /// The bug this replaces: taking the FIRST hop let a caller send its own `X-Forwarded-For` and
    /// choose which rate-limit bucket to land in — a fresh one per attempt defeats the limiter
    /// entirely. Only the last hop is written by our proxy.
    #[test]
    fn a_spoofed_forwarded_for_cannot_pick_its_own_bucket() {
        let spoofed = h(&[("x-forwarded-for", "1.2.3.4, 203.0.113.9")]);
        assert_eq!(enroll_client(&spoofed), "203.0.113.9");

        // Two attempts claiming different origins must still share a bucket.
        let a = h(&[("x-forwarded-for", "10.0.0.1, 203.0.113.9")]);
        let b = h(&[("x-forwarded-for", "10.0.0.2, 203.0.113.9")]);
        assert_eq!(enroll_client(&a), enroll_client(&b));
    }

    #[test]
    fn x_real_ip_wins_when_the_proxy_sets_it() {
        let m = h(&[
            ("x-real-ip", "203.0.113.9"),
            ("x-forwarded-for", "1.2.3.4, 5.6.7.8"),
        ]);
        assert_eq!(enroll_client(&m), "203.0.113.9");
    }

    #[test]
    fn a_single_hop_is_used_as_is() {
        assert_eq!(
            enroll_client(&h(&[("x-forwarded-for", "203.0.113.9")])),
            "203.0.113.9"
        );
    }

    /// No headers must not mean "no limit" — everyone shares one bucket, which is stricter.
    #[test]
    fn a_missing_header_shares_one_strict_bucket() {
        assert_eq!(enroll_client(&h(&[])), "unknown");
        assert_eq!(enroll_client(&h(&[("x-forwarded-for", "  ")])), "unknown");
    }
}
