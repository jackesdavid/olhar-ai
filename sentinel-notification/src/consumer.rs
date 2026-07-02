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

    // Download any media the object store holds for this row.
    let image = job.image_key.as_deref().and_then(|k| fetch_media(&job.id, k));
    let video = job.video_key.as_deref().and_then(|k| fetch_media(&job.id, k));

    // A direct bot reply (card #15) carries a raw chat_id → send straight to that chat, bypassing
    // beacon resolution. Otherwise resolve the tenant's beacon target(s) as before.
    let result = match job.chat_id {
        Some(chat_id) => send_to_chat(&cfg, chat_id, &job.message, image, video),
        None => {
            let store: NotificationStore =
                Arc::new(Mutex::new(repo::load_beacons(&state.db, job.tenant_id)));
            notification::dispatch(&store, &cfg, job.target.as_deref(), &job.message, image)
        }
    };
    match result {
        Ok(result) => queue::mark_sent(&state.db, job.id, &result),
        Err(e) => queue::mark_retry_or_fail(&state.db, job.id, job.attempts, MAX_ATTEMPTS, backoff, &e),
    }
}

/// Fetch an object-store blob, logging (and swallowing) a miss so a bad key doesn't wedge the row.
fn fetch_media(job_id: &uuid::Uuid, key: &str) -> Option<Vec<u8>> {
    match app_core::media::get(key) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            log::error!("notification {job_id}: media get {key} failed: {e}");
            None
        }
    }
}

/// Send directly to a specific Telegram chat (a bot reply). Prefers video, then photo, then text —
/// a message may carry a caption alongside its media.
fn send_to_chat(
    cfg: &notification::NotificationConfig,
    chat_id: i64,
    message: &str,
    image: Option<Vec<u8>>,
    video: Option<Vec<u8>>,
) -> Result<String, String> {
    use app_core::notification::client;
    if let Some(mp4) = video {
        client::send_video(cfg, chat_id, message, &mp4)?;
        Ok(format!("Sent video to chat {chat_id}."))
    } else if let Some(jpeg) = image {
        client::send_photo(cfg, chat_id, message, &jpeg)?;
        Ok(format!("Sent photo to chat {chat_id}."))
    } else {
        client::send_message(cfg, chat_id, message)?;
        Ok(format!("Sent message to chat {chat_id}."))
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
