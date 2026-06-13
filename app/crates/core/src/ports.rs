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
use crate::agent::detector::PersonResult;
use crate::db::Db;
use crate::devices::Device;
use crate::notification::{NotificationConfig, NotificationStore};
use crate::persons::RecognitionMatch;

/// Capture a still frame from a camera. The local impl grabs from the shared
/// go2rtc feed (fast) and falls back to ONVIF/ffmpeg.
pub trait FrameSource: Send + Sync {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String>;
}

/// Local computer-vision over a frame: person detection + a vision-model caption.
/// Both are stateless and read-only — trivially relocatable to the edge.
pub trait Vision: Send + Sync {
    fn detect_persons(&self, jpeg: &[u8]) -> Result<PersonResult, String>;
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

/// Frames from the local go2rtc feed / ONVIF fallback.
pub struct LocalFrameSource;
impl FrameSource for LocalFrameSource {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        crate::device_actions::capture_snapshot_bytes(device)
    }
}

/// Vision via the bundled local models (tract YOLOX + the vision client).
pub struct LocalVision;
impl Vision for LocalVision {
    fn detect_persons(&self, jpeg: &[u8]) -> Result<PersonResult, String> {
        crate::agent::detector::detect_person(jpeg)
    }
    fn describe(&self, jpeg: &[u8], label: &str) -> Result<String, String> {
        crate::agent::vision::describe_image(jpeg, label)
    }
}

/// Recognition via the shared Postgres + pgvector handle.
pub struct LocalRecognizer {
    pub db: Db,
}
impl Recognizer for LocalRecognizer {
    fn recognize(&self, embedding: &[f32], k: i64) -> Vec<RecognitionMatch> {
        crate::persons::repo::recognize(&self.db, embedding, k)
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
