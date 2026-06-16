//! Boundary ports (Phase 0.6) — the trait seams behind the in-process calls that
//! cross a tier boundary in the cloud decomposition. Today every port has a
//! **local** (in-process) implementation that just calls the existing function, so
//! behavior is unchanged. Later phases swap an individual port for a REST client
//! (binary JPEG over `GET`, idempotency-keyed mutations) **without touching the
//! call-sites** — nodes and tools reach the world only through `ToolContext`, which
//! holds these as trait objects.
//!
//! | Port | Local impl wraps | Later becomes |
//! |---|---|---|
//! | [`FrameSource`] | `device_actions::capture_snapshot_bytes` | Edge Gateway snapshot |
//! | [`Vision`] | `agent::detector` / `agent::vision` | Edge Gateway vision |
//! | [`Recognizer`] | `persons::repo::recognize` | REST kNN |
//! | [`Notifier`] | `notification::dispatch` | Telegram ingress |
use crate::agent::detector::{PersonDetection, PersonResult};
use crate::db::Db;
use crate::devices::Device;
use crate::notification::{NotificationConfig, NotificationStore};
use crate::persons::RecognitionMatch;

/// Capture a still frame from a camera. The local impl grabs from the shared
/// go2rtc feed (fast) and falls back to ONVIF/ffmpeg.
pub trait FrameSource: Send + Sync {
    /// On-demand still capture — best freshness/quality; may connect a cold camera.
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String>;

    /// The most-recent already-acquired frame (~zero latency) plus its age in ms. Warms a
    /// background grabber for this camera (see [`crate::frames`]) and returns its latest
    /// frame within `max_age_ms`, falling back to [`Self::snapshot`] when no warm frame is
    /// available yet. Default impl has no cache → always a fresh `snapshot` at age 0.
    fn current(&self, device: &Device, _max_age_ms: u64) -> Result<(Vec<u8>, u64), String> {
        Ok((self.snapshot(device)?, 0))
    }
}

/// Local computer-vision over a frame: person detection + a vision-model caption.
/// Both are stateless and read-only — trivially relocatable to the edge.
pub trait Vision: Send + Sync {
    fn detect_persons(&self, jpeg: &[u8]) -> Result<PersonResult, String>;
    /// Per-person detections (box + confidence) — backs the `PersonDetection[]` output.
    fn detect_persons_detailed(&self, jpeg: &[u8]) -> Result<Vec<PersonDetection>, String>;
    fn describe(&self, jpeg: &[u8], label: &str) -> Result<String, String>;
}

/// Face recognition: a cosine kNN over the approved enrolled embeddings.
pub trait Recognizer: Send + Sync {
    fn recognize(&self, embedding: &[f32], k: i64) -> Vec<RecognitionMatch>;
}

/// Outbound notification dispatch (resolve a target + send). The store + config are
/// passed in (data-level injection) so the local impl stays stateless.
pub trait Notifier: Send + Sync {
    fn dispatch(
        &self,
        store: &NotificationStore,
        cfg: &NotificationConfig,
        target: Option<&str>,
        message: &str,
        image: Option<Vec<u8>>,
    ) -> Result<String, String>;
}

// ---- local (in-process) implementations — today's behavior ------------------

/// Frames from the local go2rtc feed / ONVIF fallback (localhost). Snapshot-only:
/// `current()` uses the trait default (a fresh snapshot). The default for the desktop shell
/// and tests, where go2rtc runs on `127.0.0.1`.
pub struct LocalFrameSource;
impl FrameSource for LocalFrameSource {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        crate::device_actions::capture_snapshot_bytes(device)
    }
}

/// Frames from a SPECIFIC go2rtc `base` (a tenant's edge), with a per-instance keep-alive
/// agent. Built once per workflow run and held in that run's `ToolContext.frames`, so every
/// `GetFrame` / `Snapshot` call in the run reuses the **warm** connection and gets a **fresh**
/// frame each call (a `memoize=false` node in a loop re-grabs without re-setup). Nothing is
/// pod-shared, so concurrent runs of different tenants never collide — each targets its own
/// edge. Dropped when the run ends (connection closed).
pub struct EdgeFrameSource {
    base: String,
    agent: ureq::Agent,
}
impl EdgeFrameSource {
    pub fn new(base: impl Into<String>) -> Self {
        EdgeFrameSource {
            base: base.into(),
            agent: ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(5)).build(),
        }
    }
}
impl FrameSource for EdgeFrameSource {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        crate::go2rtc::frame_from(&self.agent, &self.base, &device.ip)
    }
    fn current(&self, device: &Device, _max_age_ms: u64) -> Result<(Vec<u8>, u64), String> {
        Ok((self.snapshot(device)?, 0))
    }
}

/// Vision via the bundled local models (tract YOLOX + the vision client).
pub struct LocalVision;
impl Vision for LocalVision {
    fn detect_persons(&self, jpeg: &[u8]) -> Result<PersonResult, String> {
        crate::agent::detector::detect_person(jpeg)
    }
    fn detect_persons_detailed(&self, jpeg: &[u8]) -> Result<Vec<PersonDetection>, String> {
        crate::agent::detector::detect_persons_detailed(jpeg)
    }
    fn describe(&self, jpeg: &[u8], label: &str) -> Result<String, String> {
        crate::agent::vision::describe_image(jpeg, label)
    }
}

/// Recognition via the shared Postgres + pgvector handle, scoped to a tenant.
pub struct LocalRecognizer {
    pub db: Db,
    pub tenant_id: uuid::Uuid,
}
impl Recognizer for LocalRecognizer {
    fn recognize(&self, embedding: &[f32], k: i64) -> Vec<RecognitionMatch> {
        crate::persons::repo::recognize(&self.db, self.tenant_id, embedding, k)
    }
}

/// Notifications via the in-process Telegram dispatch.
pub struct LocalNotifier;
impl Notifier for LocalNotifier {
    fn dispatch(
        &self,
        store: &NotificationStore,
        cfg: &NotificationConfig,
        target: Option<&str>,
        message: &str,
        image: Option<Vec<u8>>,
    ) -> Result<String, String> {
        crate::notification::dispatch(store, cfg, target, message, image)
    }
}
