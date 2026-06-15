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
mod domain;
mod handlers;
mod http;
mod oauth;
mod rbac;
mod seed;
mod store;
mod tailscale;

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
    /// Tailscale enrollment broker (None → tenants supply their own auth key).
    pub tailscale: Option<tailscale::TailscaleConfig>,
    /// Shared HTTP client (Tailscale key minting, etc.).
    pub http: reqwest::Client,
    /// Camera credentials in transit: the web sets them, the edge collects them on its
    /// next config pull, then they're discarded. Keyed by (tenant_id, camera ip) →
    /// (username, password, set-at). **Never persisted** — camera creds never touch the DB.
    pub pending_creds: Arc<Mutex<HashMap<(uuid::Uuid, String), (String, String, Instant)>>>,
    /// Per-tenant config-change notifiers — the edge long-polls /config/cameras and is
    /// woken the instant the web changes a camera (so edits apply in ~1s, not on a timer).
    pub notifiers: Arc<Mutex<HashMap<uuid::Uuid, Arc<tokio::sync::Notify>>>>,
}

impl AppState {
    /// The change-notifier for a tenant (created on first use).
    pub fn notifier(&self, tid: uuid::Uuid) -> Arc<tokio::sync::Notify> {
        self.notifiers.lock().unwrap().entry(tid).or_default().clone()
    }
    /// Wake any edge long-polling this tenant's config (a camera was added/edited).
    pub fn bump(&self, tid: uuid::Uuid) {
        self.notifier(tid).notify_waiters();
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://sentinel:sentinel@localhost/sentinel".to_string());
    let bind = std::env::var("CONTROL_BIND").unwrap_or_else(|_| "0.0.0.0:8790".to_string());
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        log::warn!("JWT_SECRET unset — using an insecure dev secret");
        "dev-insecure-secret-change-me".to_string()
    });

    // Boot synchronously (Db::connect drives its own runtime + runs migrations).
    let db = boot_db(&database_url);

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    rt.block_on(async move {
        // This service's own async pool (auth/tenancy queries run here).
        let pool = match PgPoolOptions::new().max_connections(5).connect(&database_url).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("connect auth pool: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = seed::run(&pool).await {
            log::error!("seed failed: {e}");
        }

        let google = oauth::GoogleOauth::from_env();
        log::info!("Google login: {}", if google.is_some() { "enabled" } else { "disabled (set GOOGLE_CLIENT_ID/SECRET)" });
        let oauth_states = Arc::new(Mutex::new(HashMap::new()));

        let tailscale = tailscale::TailscaleConfig::from_env();
        log::info!(
            "Tailscale edge enrollment: {}",
            if tailscale.is_some() { "enabled (auto-mint auth keys)" } else { "disabled (tenants supply their own key)" }
        );
        let http = reqwest::Client::new();

        // Face recognition (enroll + recognize) needs the AI sidecar. Log its config at
        // boot — like Google/Tailscale above — so a missing AI_SERVICE_URL is obvious
        // instead of only surfacing as a per-request "not configured" error.
        match app_core::agent::ai_service::AiServiceConfig::from_env() {
            Some(cfg) => log::info!("AI recognition: enabled (sidecar {})", cfg.url),
            None => log::warn!("AI recognition: disabled — set AI_SERVICE_URL (+ AI_SERVICE_KEY) to enable enroll/recognize"),
        }

        let pending_creds = Arc::new(Mutex::new(HashMap::new()));
        let notifiers = Arc::new(Mutex::new(HashMap::new()));
        let state = AppState { db, pool, jwt_secret, google, oauth_states, tailscale, http, pending_creds, notifiers };
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
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
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
