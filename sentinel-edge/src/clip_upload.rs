//! Ship finished clips to durable cloud storage (card #64).
//!
//! A clip lives on this box's local ring. If the box is stolen or smashed, the recording of
//! that happening goes with it — so when the tenant's plan sells cloud recording, control
//! answers the clip manifest with a short-lived **presigned PUT**. The bytes go straight from
//! here to R2: no video crosses the VPS, and the edge never holds a cloud credential.
//!
//! **One upload at a time.** This box is also serving live video over the same uplink, and a
//! burst of parallel PUTs after a busy hour would stall it. Jobs queue on a bounded channel
//! and a single consumer drains them; a dropped job only ever costs durability, never the clip.
//!
//! **Eviction** (card #78, opt-in via `recording_config.delete_local_after_upload`): once — and
//! only once — control has confirmed the clip is durable, the local `.mp4` and its sidecar are
//! deleted, so the appliance's disk is a transient spool rather than a growing archive. Off by
//! default: for a tenant on a plan without cloud recording nothing is ever uploaded, and the local
//! file is the only copy there is.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::time::Duration;

/// One clip waiting to become durable.
pub struct Job {
    pub clip_id: String,
    pub path: PathBuf,
    /// The presigned PUT control minted for this clip. Expires (control's `UPLOAD_URL_TTL_SECS`,
    /// currently 1 h), which is what bounds our retries.
    pub url: String,
    /// Drop the local copy once the clip is durable — this box's `delete_local_after_upload`.
    pub delete_local: bool,
}

/// Bounded, so a stalled uplink can't grow the queue without limit.
const QUEUE_DEPTH: usize = 64;

/// Waits between attempts, in seconds. Four tries over ~6 min — well inside the presign TTL.
const BACKOFF: [u64; 3] = [10, 60, 300];

/// A clip larger than this means something is wrong (a workflow driving the box to record
/// continuously). The whole local ring defaults to 1 GB, so this should be unreachable;
/// refuse rather than saturate the customer's uplink for an hour.
const MAX_CLIP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Start the single-consumer upload worker. Returns the handle jobs are pushed onto.
pub fn spawn(control_url: String) -> SyncSender<Job> {
    let (tx, rx) = sync_channel(QUEUE_DEPTH);
    std::thread::Builder::new()
        .name("clip-upload".to_string())
        .spawn(move || worker(control_url, rx))
        .expect("spawn clip-upload thread");
    tx
}

/// Hand a clip to the worker. Never blocks the caller (the HTTP handler that just stopped a
/// recording): a full queue drops the job and says so.
pub fn enqueue(tx: &SyncSender<Job>, job: Job) {
    match tx.try_send(job) {
        Ok(()) => {}
        Err(TrySendError::Full(j)) => {
            log::warn!("clip {}: upload queue full, staying local", j.clip_id)
        }
        Err(TrySendError::Disconnected(j)) => {
            log::warn!("clip {}: uploader stopped, staying local", j.clip_id)
        }
    }
}

fn worker(control_url: String, rx: Receiver<Job>) {
    for job in rx {
        if let Err(e) = upload(&job) {
            log::warn!("clip {}: upload gave up: {}", job.clip_id, e.msg);
            continue;
        }
        // The bytes are durable but the row still reads `uploading`. If this never lands,
        // control's sweep marks the clip `failed` and R2's lifecycle expires the object.
        if let Err(e) = confirm(&control_url, &job.clip_id) {
            log::warn!(
                "clip {}: uploaded, but confirm failed: {}",
                job.clip_id,
                e.msg
            );
            continue;
        }
        log::info!("clip {} is durable", job.clip_id);
        // Only now — a clip whose confirm failed still reads `uploading` cloud-side and may yet be
        // marked `failed`, so the local file stays its safety net.
        if job.delete_local {
            evict(&job);
        }
    }
}

/// Drop the local `.mp4` **and its `.json` sidecar**. Both: `StorageStats::compute` and the
/// storage evictor read `size_bytes` out of the sidecar, so an orphaned one would make this box
/// over-report its disk usage forever.
fn evict(job: &Job) {
    if let Err(e) = std::fs::remove_file(&job.path) {
        log::warn!(
            "clip {}: uploaded, but could not drop the local copy: {e}",
            job.clip_id
        );
        return; // leave the sidecar: it still describes a file that is really there
    }
    let _ = std::fs::remove_file(job.path.with_extension("json"));
    log::info!(
        "clip {}: local copy dropped (it lives in the cloud now)",
        job.clip_id
    );
}

fn upload(job: &Job) -> Result<(), Failure> {
    let len = std::fs::metadata(&job.path)
        .map_err(|e| Failure::permanent(format!("stat {:?}: {e}", job.path)))?
        .len();
    if len == 0 {
        return Err(Failure::permanent("clip file is empty".to_string()));
    }
    if len > MAX_CLIP_BYTES {
        return Err(Failure::permanent(format!(
            "clip is {len} bytes, above the {MAX_CLIP_BYTES}-byte upload cap"
        )));
    }
    retry("upload", &job.clip_id, || put(&job.url, &job.path, len))
}

fn put(url: &str, path: &Path, len: u64) -> Result<(), Failure> {
    let file =
        std::fs::File::open(path).map_err(|e| Failure::permanent(format!("open {path:?}: {e}")))?;
    // Set `Content-Length` ourselves. Handed an unsized reader, ureq falls back to
    // `Transfer-Encoding: chunked`, which S3/R2 rejects on a presigned PUT. With the header
    // present it streams the file straight through, so a clip never sits in memory.
    status(
        agent()
            .put(url)
            .set("Content-Type", "video/mp4")
            .set("Content-Length", &len.to_string())
            .send(file),
    )
}

fn confirm(control_url: &str, clip_id: &str) -> Result<(), Failure> {
    let url = format!("{control_url}/config/recordings/{clip_id}/uploaded");
    retry("confirm", clip_id, || {
        let mut req = agent().post(&url);
        if let Ok(t) = std::env::var("BOX_TOKEN") {
            let t = t.trim();
            if !t.is_empty() {
                req = req.set("Authorization", &format!("Bearer {t}"));
            }
        }
        status(req.call())
    })
}

/// Uploads are long (a clip over a home uplink); the confirm that follows is a single small
/// POST. One generous timeout covers both.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(300))
        .timeout_write(Duration::from_secs(600))
        .build()
}

struct Failure {
    retryable: bool,
    msg: String,
}

impl Failure {
    fn permanent(msg: String) -> Self {
        Failure {
            retryable: false,
            msg,
        }
    }
    fn transient(msg: String) -> Self {
        Failure {
            retryable: true,
            msg,
        }
    }
}

/// A rejected PUT is not a hiccup: an expired presign (403) or a bad key (404) will be rejected
/// identically four times, and the worker is single-consumer — six minutes spent re-sending a
/// doomed clip is six minutes the next clip waits. Only server faults, timeouts, and throttling
/// earn a retry.
fn status(resp: Result<ureq::Response, ureq::Error>) -> Result<(), Failure> {
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, r)) => {
            let detail: String = r
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect();
            let msg = format!("HTTP {code} {detail}");
            let retryable = code >= 500 || code == 408 || code == 429;
            if retryable {
                Err(Failure::transient(msg))
            } else {
                Err(Failure::permanent(msg))
            }
        }
        Err(e) => Err(Failure::transient(e.to_string())), // transport: worth another try
    }
}

fn retry(
    what: &str,
    clip_id: &str,
    mut f: impl FnMut() -> Result<(), Failure>,
) -> Result<(), Failure> {
    let mut attempt = 0;
    loop {
        match f() {
            Ok(()) => return Ok(()),
            Err(e) if !e.retryable => return Err(e),
            Err(e) => match BACKOFF.get(attempt) {
                None => return Err(e),
                Some(wait) => {
                    log::debug!(
                        "clip {clip_id}: {what} attempt {} failed ({}), retrying in {wait}s",
                        attempt + 1,
                        e.msg
                    );
                    std::thread::sleep(Duration::from_secs(*wait));
                    attempt += 1;
                }
            },
        }
    }
}
