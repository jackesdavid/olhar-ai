//! The queue consumer: claim a due notification, send it, mark it terminal (or requeue with
//! backoff). Runs on a std thread — the repo functions bridge sqlx via `Db::block`, so no
//! tokio context is needed here (mirrors flow's engine poller).
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_core::notification::{self, queue, repo, NotificationConfig, NotificationStore};

use crate::state::AppState;

const MAX_ATTEMPTS: i32 = 5;
/// Idle poll interval when the queue is empty. (A future Postgres LISTEN/NOTIFY ping can
/// wake this instantly; polling is the durable fallback.)
const IDLE_POLL: Duration = Duration::from_millis(750);

pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || loop {
        match queue::claim_next(&state.db) {
            Some(job) => process(&state, job),
            None => std::thread::sleep(IDLE_POLL),
        }
    });
    log::info!("notification queue consumer started");
}

fn process(state: &AppState, job: queue::QueuedNotification) {
    let backoff = backoff_secs(job.attempts);
    let Some(cfg) = resolve_config(&state.db, job.tenant_id) else {
        queue::mark_retry_or_fail(
            &state.db,
            job.id,
            job.attempts,
            MAX_ATTEMPTS,
            backoff,
            "no Telegram configuration for this tenant",
        );
        return;
    };

    // Download the image from the object store (if one rode this notification).
    let image = job.image_key.as_deref().and_then(|k| match app_core::media::get(k) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            log::error!("notification {}: media get {k} failed: {e}", job.id);
            None
        }
    });
    // Video attachments (recordings) arrive with #15's Reply-to-sender; text/image go out now.
    if job.video_key.is_some() {
        log::warn!("notification {}: video attachment present — video send lands with #15", job.id);
    }

    let store: NotificationStore = Arc::new(Mutex::new(repo::load_beacons(&state.db, job.tenant_id)));
    match notification::dispatch(&store, &cfg, job.target.as_deref(), &job.message, image) {
        Ok(result) => queue::mark_sent(&state.db, job.id, &result),
        Err(e) => queue::mark_retry_or_fail(&state.db, job.id, job.attempts, MAX_ATTEMPTS, backoff, &e),
    }
}

/// Resolve the tenant's send config: tenant token → global token → `TELEGRAM_BOT_TOKEN` env.
fn resolve_config(db: &app_core::db::Db, tenant: uuid::Uuid) -> Option<NotificationConfig> {
    let (mut token, code) = repo::load_telegram(db, tenant).unwrap_or_default();
    if token.trim().is_empty() {
        token = repo::load_global_telegram(db)
            .map(|(t, _)| t)
            .filter(|t| !t.trim().is_empty())
            .or_else(|| std::env::var("TELEGRAM_BOT_TOKEN").ok())
            .unwrap_or_default();
    }
    if token.trim().is_empty() {
        return None;
    }
    Some(NotificationConfig { bot_token: token, pairing_code: code })
}

/// Exponential backoff, capped at 5 minutes: attempt 1 → 5s, 2 → 10s, 3 → 20s, …
fn backoff_secs(attempts: i32) -> i64 {
    let base = 5i64.saturating_mul(1i64 << attempts.clamp(0, 8).max(1).saturating_sub(1));
    base.min(300)
}
