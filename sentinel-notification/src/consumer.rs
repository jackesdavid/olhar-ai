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

/// How often staged media is reclaimed.
const PURGE_INTERVAL: Duration = Duration::from_secs(300);
/// How long a terminal notification's media survives, so an operator can still inspect a
/// just-failed send before its attachment disappears.
const PURGE_GRACE_SECS: i64 = 3600;
/// Objects reclaimed per pass — bounded so a backlog can't monopolise the thread.
const PURGE_BATCH: i64 = 200;
/// Backstop expiry for objects nothing points at (staged, then the enqueue failed).
const ORPHAN_EXPIRY_DAYS: u32 = 3;

pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || loop {
        match queue::claim_next(&state.db) {
            Some(job) => process(&state, job),
            None => std::thread::sleep(IDLE_POLL),
        }
    });
    log::info!("notification queue consumer started");
}

/// Reclaim staged media.
///
/// The object store is a hand-off between features, not durable storage: once a notification
/// is terminal its snapshot/clip has no reader. Nothing deleted them, so the bucket grew
/// forever on the same disk as Postgres — an eventual outage of the whole platform, not just
/// notifications. The bucket lifecycle rule is a backstop for orphans (an object staged for a
/// send that never enqueued); this is the prompt path.
pub fn spawn_media_purge(state: Arc<AppState>) {
    if !app_core::media::enabled() {
        log::info!("media purge: object store not configured — nothing to reclaim");
        return;
    }
    match app_core::media::ensure_lifecycle(ORPHAN_EXPIRY_DAYS) {
        Ok(()) => log::info!("media purge: bucket expires orphans after {ORPHAN_EXPIRY_DAYS}d"),
        Err(e) => log::warn!("media purge: could not set lifecycle ({e}) — prompt reclaim still runs"),
    }
    std::thread::spawn(move || loop {
        let batch = queue::purgeable_media(&state.db, PURGE_GRACE_SECS, PURGE_BATCH);
        let mut reclaimed = Vec::new();
        for row in &batch {
            // Only forget the keys once every object for that row is actually gone, or a
            // failure would leave the object orphaned with nothing pointing at it.
            let image_gone = row.image_key.as_deref().map(drop_object).unwrap_or(true);
            let video_gone = row.video_key.as_deref().map(drop_object).unwrap_or(true);
            if image_gone && video_gone {
                reclaimed.push(row.id);
            }
        }
        if !reclaimed.is_empty() {
            let n = queue::clear_media_keys(&state.db, &reclaimed);
            log::info!("media purge: reclaimed media for {n} notification(s)");
        }
        std::thread::sleep(PURGE_INTERVAL);
    });
    log::info!("media purge started (every {}s, {}s grace)", PURGE_INTERVAL.as_secs(), PURGE_GRACE_SECS);
}

/// Delete one object. S3 deletes are idempotent, so an already-missing key is a success.
fn drop_object(key: &str) -> bool {
    match app_core::media::delete(key) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("media purge: could not delete '{key}': {e}");
            false
        }
    }
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
