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

/// Frames from the local go2rtc feed / ONVIF fallback. Snapshot-only: `current()` uses
/// the trait default (a fresh snapshot), so there's no warm cache. The safe default for
/// tests and any caller that doesn't own a pod [`FrameHub`].
pub struct LocalFrameSource;
impl FrameSource for LocalFrameSource {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        crate::device_actions::capture_snapshot_bytes(device)
    }
}

/// A frame source backed by a pod-owned [`FrameHub`] (see [`crate::frames`]). `current()`
/// returns the camera's warm in-memory frame (~zero latency), keeping a background grabber
/// alive; `snapshot()` is still the on-demand path. The pod creates ONE hub at boot and
/// injects this into every run's `ToolContext` (sharing the `Arc`), so all runs/graphs/
/// instances in the pod share one grabber per camera.
pub struct WarmFrameSource {
    pub hub: std::sync::Arc<crate::frames::FrameHub>,
}
impl FrameSource for WarmFrameSource {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        crate::device_actions::capture_snapshot_bytes(device)
    }

    fn current(&self, device: &Device, max_age_ms: u64) -> Result<(Vec<u8>, u64), String> {
        let max_age = std::time::Duration::from_millis(max_age_ms.max(1));
        if let Some((bytes, age)) = self.hub.current(&device.ip, max_age) {
            return Ok(((*bytes).clone(), age.as_millis() as u64));
        }
        // Cache cold (first read) → on-demand snapshot; the grabber warms for next time.
        Ok((self.snapshot(device)?, 0))
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
