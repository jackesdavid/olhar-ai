//! `sentinel-control` — the control plane / SaaS backend.
//!
//! Owns the central source of truth + multi-tenant auth: tenants, users, RBAC
//! (configurable, action-based), JWT sessions (+ Google/Apple later), and the camera
//! config edge appliances pull. Authorization is permission-based — see `auth.rs`.
//!
//! Run:
//!   DATABASE_URL=postgres://… JWT_SECRET=… CONTROL_BIND=0.0.0.0:8790 \
//!   cargo run -p sentinel-control   # or: task control
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use app_core::db::Db;

mod auth;
mod billing_gate;
mod box_signing;
mod clip_purge;
mod domain;
mod edge_stream;
mod handlers;
mod http;
mod oauth;
mod rbac;
mod seed;
mod seed_templates;
mod store;
mod webauthn;

/// Shared state. `db` (app-core) owns migrations + the domain repos; `pool` is this
/// service's own async pool for auth/tenancy queries (runs on this runtime).
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub pool: PgPool,
    pub jwt_secret: String,
    /// Google OAuth config (None when unconfigured — social login disabled).
    pub google: Option<oauth::GoogleOauth>,
    /// In-flight OAuth CSRF states (state → issued-at), pruned on use.
    pub oauth_states: Arc<Mutex<HashMap<String, Instant>>>,
    /// Shared HTTP client (edge/flow proxying, external APIs).
    pub http: reqwest::Client,
    /// Camera credentials in transit: the web sets them, the edge collects them on its
    /// next config pull, then they're discarded. Keyed by (tenant_id, camera ip) →
    /// (username, password, set-at). **Never persisted** — camera creds never touch the DB.
    pub pending_creds: Arc<Mutex<HashMap<(uuid::Uuid, String), (String, String, Instant)>>>,
    /// Per-tenant config-change notifiers — the edge long-polls /config/cameras and is
    /// woken the instant the web changes a camera (so edits apply in ~1s, not on a timer).
    pub notifiers: Arc<Mutex<HashMap<uuid::Uuid, Arc<tokio::sync::Notify>>>>,
    /// Tenants with a pending "rescan cameras" command — set by the UI, consumed by the
    /// edge on its next poll (see /config/commands). Rescan is idempotent + cheap.
    pub pending_rescan: Arc<Mutex<std::collections::HashSet<uuid::Uuid>>>,
    // pending_restart / pending_update are now persisted in tenant_boxes (migration 0031)
    // so they survive control restarts. See store::set_box_command / take_box_commands.
    /// Unified platform audit log queue (async, fire-and-forget). All modules push here;
    /// the drain task batch-inserts into platform_logs every 500 ms.
    pub log_queue: app_core::platform_logs::LogQueue,
    /// Agnostic product-event stream queue (async, fire-and-forget) — card #66. Subsystems push
    /// occurrences here; the drain batch-inserts into `events`. The debug trace is `log_queue`.
    pub event_queue: app_core::events::EventQueue,
    /// Live cloud→edge SSE command streams this pod holds (`box_id → connection`). The durable
    /// queue is `app_core::edge_commands`; this is the connection half. Card #144.
    pub edge_streams: edge_stream::EdgeStreams,
}

impl AppState {
    /// The change-notifier for a tenant (created on first use).
    pub fn notifier(&self, tid: uuid::Uuid) -> Arc<tokio::sync::Notify> {
        self.notifiers
            .lock()
            .unwrap()
            .entry(tid)
            .or_default()
            .clone()
    }
    /// Wake any edge long-polling this tenant's config (a camera was added/edited).
    pub fn bump(&self, tid: uuid::Uuid) {
        self.notifier(tid).notify_waiters();
    }
    /// Queue a rescan for the tenant's edge and wake its long-poll so it acts now.
    pub fn request_rescan(&self, tid: uuid::Uuid) {
        self.pending_rescan.lock().unwrap().insert(tid);
        self.bump(tid);
    }
    /// Consume the tenant's pending rescan flag (true if one was queued).
    pub fn take_rescan(&self, tid: uuid::Uuid) -> bool {
        self.pending_rescan.lock().unwrap().remove(&tid)
    }
    /// Queue a restart for a specific box (persisted to DB — survives control restarts).
    pub async fn request_restart(&self, box_id: uuid::Uuid) {
        if let Err(e) =
            crate::store::set_box_command(&self.pool, box_id, "pending_restart", true).await
        {
            log::warn!("set pending_restart: {e}");
        }
    }
    /// Queue an update for a specific box (persisted to DB — survives control restarts).
    pub async fn request_update(&self, box_id: uuid::Uuid) {
        if let Err(e) =
            crate::store::set_box_command(&self.pool, box_id, "pending_update", true).await
        {
            log::warn!("set pending_update: {e}");
        }
    }
    /// Atomically read and clear both command flags for a box (called on the edge's poll).
    /// Returns (restart, update).
    pub async fn take_box_commands(&self, box_id: uuid::Uuid) -> (bool, bool) {
        crate::store::take_box_commands(&self.pool, box_id)
            .await
            .unwrap_or((false, false))
    }
}

/// The dev-only fallback secret. Anyone can read it here, which is exactly why a release build must
/// never reach for it.
const DEV_JWT_SECRET: &str = "dev-insecure-secret-change-me";

/// Resolve the JWT signing secret, or refuse to boot.
///
/// This secret mints every session, including `master: true` — a token forged with a known value is
/// total compromise of every tenant. The old behaviour fell back to a literal from this file with
/// only a `log::warn!`, so one missing env var in production silently published the keys.
///
/// The escape hatch is a property of the BINARY, not of the environment (same shape as
/// `untenanted_dev_config()` in `http.rs`): a release build cannot be talked into dev behaviour by
/// anything set on the host. Crashing is the point — a control plane that cannot authenticate
/// safely should not serve, and a boot loop is far louder than a warning nobody reads.
fn resolve_jwt_secret(configured: Option<String>) -> Result<String, &'static str> {
    match configured {
        // 32 chars is not cryptographic rigour, it is a typo guard: it rejects "changeme" and the
        // half-pasted value, which is the realistic failure.
        Some(s) if s.trim().len() >= 32 => Ok(s),
        Some(_) if cfg!(debug_assertions) => Ok(DEV_JWT_SECRET.to_string()),
        Some(_) => Err("JWT_SECRET is set but shorter than 32 characters"),
        None if cfg!(debug_assertions) => {
            log::warn!("JWT_SECRET unset — using the insecure dev secret (debug build only)");
            Ok(DEV_JWT_SECRET.to_string())
        }
        None => Err("JWT_SECRET is unset"),
    }
}

fn main() {
    // Structured logging → Datadog (card #146): every log ships with service:control + span context.
    app_core::datadog::init_tracing("control");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://sentinel:sentinel@localhost/sentinel".to_string());
    let bind = std::env::var("CONTROL_BIND").unwrap_or_else(|_| "0.0.0.0:8790".to_string());
    let jwt_secret = match resolve_jwt_secret(std::env::var("JWT_SECRET").ok()) {
        Ok(s) => s,
        Err(why) => {
            log::error!("refusing to start: {why}");
            std::process::exit(1);
        }
    };

    // Boot synchronously (Db::connect drives its own runtime + runs migrations).
    let db = boot_db(&database_url);

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    rt.block_on(async move {
        // This service's own async pool (auth/tenancy + every domain query runs here). 5 was far too
        // small for the whole API backend under real concurrency (SSE + edge polling + the UI).
        let pool = match PgPoolOptions::new().max_connections(20).connect(&database_url).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("connect auth pool: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = seed::run(&pool).await {
            log::error!("seed failed: {e}");
        }
        if let Err(e) = store::backfill_friendly_names(&pool).await {
            log::warn!("backfill_friendly_names: {e}");
        }

        let google = oauth::GoogleOauth::from_env();
        log::info!("Google login: {}", if google.is_some() { "enabled" } else { "disabled (set GOOGLE_CLIENT_ID/SECRET)" });
        let oauth_states = Arc::new(Mutex::new(HashMap::new()));

        let http = reqwest::Client::new();

        // Face recognition (enroll + recognize) needs the AI sidecar. Log its config at
        // boot — like Google above — so a missing AI_SERVICE_URL is obvious
        // instead of only surfacing as a per-request "not configured" error.
        match app_core::agent::ai_service::AiServiceConfig::from_env() {
            Some(cfg) => log::info!("AI recognition: enabled (sidecar {})", cfg.url),
            None => log::warn!("AI recognition: disabled — set AI_SERVICE_URL (+ AI_SERVICE_KEY) to enable enroll/recognize"),
        }

        // Conversational assistant (/agent/chat, /tts, /stt). The chat tool loop needs the
        // Anthropic key; TTS/STT need ElevenLabs. Log each leg so a missing key is obvious.
        log::info!("Assistant chat: {}", if app_core::agent::domain::AgentConfig::from_env().is_some() { "enabled" } else { "disabled (set ANTHROPIC_API_KEY)" });
        log::info!("Assistant voice (TTS): {}", if app_core::voice::VoiceConfig::from_env().is_some() { "enabled" } else { "disabled (set ELEVENLABS_API_KEY)" });
        log::info!("Assistant transcription (STT): {}", if app_core::listen::SttConfig::from_env().is_some() { "enabled" } else { "disabled (set ELEVENLABS_API_KEY)" });

        // Durable clip storage. Arm it only once the bucket's retention rules are proven
        // present: storing footage we cannot expire is worse than not storing it — the
        // customer is the LGPD controller and we would hold their video indefinitely.
        // Until this succeeds, `clip_store::armed()` is false and no upload URL is issued.
        if app_core::clip_store::enabled() {
            let db = db.clone();
            std::thread::spawn(move || {
                let tiers = app_core::billing::plans::retention_tiers(&db);
                match app_core::clip_store::ensure_retention_lifecycle(&tiers) {
                    Ok(()) => log::info!("clip store: armed (retention {tiers:?} days)"),
                    Err(e) => log::error!("clip store: DISARMED — retention rules not applied: {e}"),
                }
            });
        } else {
            log::info!("clip store: disabled (set R2_ENDPOINT/R2_BUCKET/R2_ACCESS_KEY_ID/R2_SECRET_ACCESS_KEY)");
        }

        // Billing FX: refresh USD→BRL (for BRL invoicing) on boot, then daily. On failure
        // the last recorded rate stays authoritative, so there is always a quote to use.
        {
            let db = db.clone();
            std::thread::spawn(move || loop {
                app_core::billing::fx::refresh_usd_brl(&db);
                std::thread::sleep(std::time::Duration::from_secs(24 * 3600));
            });
        }

        // Billing: close the previous month's invoices, daily. `close_period` returns an
        // existing `final` invoice untouched, so this is idempotent and safe to run from
        // every control replica (and re-run all month once the period is closed).
        {
            let db = db.clone();
            std::thread::spawn(move || loop {
                // Trial expiry belongs to pg_cron (migration 0046) — pure SQL, runs once
                // regardless of replica count. Invoice closing stays here: its pricing math
                // lives in `billing::invoices`, and a second copy in SQL would be a money
                // bug waiting to drift.
                let period = app_core::billing::invoices::previous_period();
                let (closed, skipped) = app_core::billing::invoices::close_period_all(&db, &period);
                log::info!("billing: period {period} — {closed} invoice(s) closed, {skipped} skipped");
                std::thread::sleep(std::time::Duration::from_secs(24 * 3600));
            });
        }

        let pending_creds = Arc::new(Mutex::new(HashMap::new()));
        let notifiers = Arc::new(Mutex::new(HashMap::new()));
        let pending_rescan = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let log_queue = app_core::platform_logs::LogQueue::spawn(pool.clone());
        let event_queue = app_core::events::EventQueue::spawn(pool.clone());

        // Drain the clip-delete work queue: edge + cloud + row, per customer deletion request.
        clip_purge::spawn(pool.clone());

        // Cloud→edge command delivery (card #144): the per-pod registry of live edge SSE streams,
        // plus the LISTEN/poll task that pushes queued commands to the boxes this pod holds.
        let edge_streams = edge_stream::new_registry();
        edge_stream::spawn_delivery(pool.clone(), edge_streams.clone());

        // The workflow live-events fabric is Postgres LISTEN/NOTIFY now (card #147): flow pods
        // `pg_notify('workflow_events', …)` and control's SSE relay LISTENs. No Redis broker.

        let state = AppState {
            db, pool, jwt_secret, google, oauth_states, http, pending_creds, notifiers,
            pending_rescan, log_queue, event_queue, edge_streams,
        };
        let app = http::router(state);
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("failed to bind {bind}: {e}");
                std::process::exit(1);
            }
        };
        log::info!("sentinel-control listening on http://{bind} (auth + RBAC + /config/cameras)");
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await {
            log::error!("server error: {e}");
            std::process::exit(1);
        }
    });
    log::info!("sentinel-control shut down");
}

fn boot_db(database_url: &str) -> Db {
    loop {
        match Db::connect(database_url) {
            Ok(db) => {
                log::info!("Postgres connected; migrations applied");
                return db;
            }
            Err(e) => {
                log::warn!("connect Postgres failed ({e}); retrying in 2s…");
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    log::info!("shutdown signal received");
}

#[cfg(test)]
mod jwt_secret_tests {
    use super::*;

    /// The failure this exists to prevent: one missing env var in production silently publishing a
    /// secret that is readable in this repository, and with it the ability to forge `master: true`.
    #[test]
    fn an_unset_secret_never_falls_back_to_the_literal_in_release() {
        let got = resolve_jwt_secret(None);
        if cfg!(debug_assertions) {
            assert_eq!(got.as_deref(), Ok(DEV_JWT_SECRET), "dev stays zero-config");
        } else {
            assert!(got.is_err(), "release must refuse to boot, not guess");
        }
    }

    /// A short value is the realistic mistake — a placeholder, or a half-pasted secret — and it is
    /// indistinguishable from a real one to everything except this check.
    #[test]
    fn a_too_short_secret_is_rejected_in_release() {
        assert_eq!(
            resolve_jwt_secret(Some("changeme".into())).is_err(),
            !cfg!(debug_assertions)
        );
    }

    #[test]
    fn a_real_secret_is_used_in_every_build() {
        let secret = "b77d6d5438011bee58f186b01296e324f8f5bea9c0f75ad6";
        assert_eq!(
            resolve_jwt_secret(Some(secret.into())).as_deref(),
            Ok(secret)
        );
    }

    /// The dev fallback must not be reachable in a release build by any input, or the escape hatch
    /// is an environment property again rather than a property of the binary.
    #[test]
    fn the_dev_secret_is_unreachable_in_release() {
        if cfg!(debug_assertions) {
            return;
        }
        for input in [None, Some(String::new()), Some("short".into())] {
            assert_ne!(resolve_jwt_secret(input).as_deref(), Ok(DEV_JWT_SECRET));
        }
    }
}
