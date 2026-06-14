//! `sentinel-control` — the control plane / SaaS backend.
//!
//! Owns the central source of truth + multi-tenant auth: tenants, users, RBAC
//! (configurable, action-based), JWT sessions (+ Google/Apple later), and the camera
//! config edge appliances pull. Authorization is permission-based — see `auth.rs`.
//!
//! Run:
//!   DATABASE_URL=postgres://… JWT_SECRET=… CONTROL_BIND=0.0.0.0:8790 \
//!   cargo run -p sentinel-control   # or: task control
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use app_core::db::Db;

mod auth;
mod handlers;
mod http;
mod rbac;
mod seed;
mod store;

/// Shared state. `db` (app-core) owns migrations + the domain repos; `pool` is this
/// service's own async pool for auth/tenancy queries (runs on this runtime).
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub pool: PgPool,
    pub jwt_secret: String,
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

        let state = AppState { db, pool, jwt_secret };
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
