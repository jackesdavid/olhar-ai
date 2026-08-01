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
    /// Per-object detections filtered to a single COCO class id — backs `ObjectDetection`.
    fn detect_objects_detailed(
        &self,
        jpeg: &[u8],
        class_id: u32,
    ) -> Result<Vec<PersonDetection>, String>;
    fn describe(&self, jpeg: &[u8], label: &str) -> Result<String, String>;
    /// Ask a yes/no question about an optional image. Returns `true` for YES, `false` for NO.
    fn ask_yes_no(&self, question: &str, image: Option<&[u8]>) -> Result<bool, String>;
    /// Single-turn free-text answer to a prompt (+ optional image). Backs the `AI Answer` node.
    fn ask_text(&self, prompt: &str, image: Option<&[u8]>) -> Result<String, String>;
    /// Route a message to one of `arms` (or `None` = no match). Backs the `AI Switch` node.
    fn classify(&self, text: &str, arms: &[String]) -> Result<Option<String>, String>;
    /// Extract `field` from a message (empty string if absent). Backs the `AI Extract` node.
    fn extract(&self, text: &str, field: &str) -> Result<String, String>;
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

    /// Reply DIRECTLY to a specific Telegram chat — a bot reply to the message sender (card #15),
    /// bypassing beacon resolution. The remote impl enqueues (durable); the local impl sends
    /// in-process. `image` optional (text-only reply when `None`).
    fn reply(
        &self,
        cfg: &NotificationConfig,
        chat_id: i64,
        message: &str,
        image: Option<Vec<u8>>,
    ) -> Result<String, String>;

    /// Reply to a chat with a video (mp4 bytes) — `Reply with recording`. Same routing as
    /// [`Self::reply`]; video always rides the object store on the remote path (it's heavy).
    fn reply_video(
        &self,
        cfg: &NotificationConfig,
        chat_id: i64,
        caption: &str,
        video: Vec<u8>,
    ) -> Result<String, String>;
}

// ---- local (in-process) implementations — today's behavior ------------------

/// `current()` uses the trait default (a fresh snapshot). The default for the desktop shell
pub struct LocalFrameSource;
impl FrameSource for LocalFrameSource {
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        crate::device_actions::capture_snapshot_bytes(device)
    }
}

/// agent. Built once per workflow run and held in that run's `ToolContext.frames`, so every
/// `GetFrame` / `Snapshot` call in the run reuses the **warm** connection and gets a **fresh**
/// frame each call (a `memoize=false` node in a loop re-grabs without re-setup). Nothing is
/// pod-shared, so concurrent runs of different tenants never collide — each targets its own
/// edge. Dropped when the run ends (connection closed).
pub struct EdgeFrameSource {
    base: String,
    agent: ureq::Agent,
    /// The SSE route to the box: `(db, tenant)`. The edge is OUTBOUND-ONLY since card #144 — there
    /// is no tailnet and `magicdns_host` is NULL — so a direct HTTP call to `base` can only reach
    /// something on the CLOUD host, which is never the edge. Every other edge caller (the probe,
    /// recordings, the tracks node) was moved to the SSE channel; this one was missed, and asked
    /// 127.0.0.1 for a frame that lives on a box across the internet.
    sse: Option<(crate::db::Db, uuid::Uuid)>,
}
/// The edge's own HTTP base on this machine — the fallback when no box is registered for a tenant.
pub fn local_base() -> String {
    std::env::var("EDGE_LOCAL_BASE").unwrap_or_else(|_| "http://127.0.0.1:8788".to_string())
}

impl EdgeFrameSource {
    pub fn new(base: impl Into<String>) -> Self {
        EdgeFrameSource {
            base: base.into(),
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(5))
                .build(),
            sse: None,
        }
    }

    /// Route frame grabs over the SSE command channel — the only path to an outbound-only box.
    /// Callers that hold a db handle and a tenant should ALWAYS use this; `new` alone can reach
    /// only a box that shares a host with the caller.
    pub fn with_sse(base: impl Into<String>, db: crate::db::Db, tenant_id: uuid::Uuid) -> Self {
        let mut s = Self::new(base);
        s.sse = Some((db, tenant_id));
        s
    }

    /// Ask the box for `ip`'s newest decoded frame over SSE. `None` = no SSE-reachable box, so the
    /// caller falls through to the direct path.
    fn sse_snapshot(&self, ip: &str) -> Option<Result<Vec<u8>, String>> {
        let (db, tenant) = self.sse.as_ref()?;
        let path = format!("/snapshot/{ip}");
        let res = crate::edge_commands::edge_http_via_sse_blocking(db, *tenant, "GET", &path, None, 15)?;
        Some(match res {
            Ok((status, body)) if (200..300).contains(&status) => Ok(body),
            Ok((status, _)) => Err(format!("edge snapshot returned {status}")),
            Err(e) => Err(e),
        })
    }
}
/// Bases whose edge API (`:8080/snapshot`) we've already found unreachable — so a box without the
/// tailnet serve (older edge) pays the probe timeout **once per process**, not on every frame grab.
fn edge_api_dead() -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    static D: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> =
        std::sync::OnceLock::new();
    D.get_or_init(Default::default)
}

/// How long a base stays written off. It used to be forever: a HashSet with no expiry, so ONE
/// timeout — a box rebooting, a blip — disabled frame grabs for the whole process lifetime, and
/// the only cure was restarting the service. The point is to avoid paying the timeout on every
/// grab, which a short cooldown achieves just as well.
const DEAD_FOR: std::time::Duration = std::time::Duration::from_secs(60);

impl EdgeFrameSource {
    /// The edge's OWN API base. A tailnet base `https://host` maps to `https://host:8080`, where
    /// the edge publishes its HTTP (8788).
    ///
    /// port, and the sidecar is gone. It stays only so a caller still holding an old base does not
    /// silently address nothing; [`local_base`] is what new callers should pass.
    fn edge_api_base(&self) -> Option<String> {
        let base = self.base.trim_end_matches('/');
        if let Some(rest) = base.strip_prefix("https://") {
            if rest.contains(':') {
                return None; // an explicit non-default port — not a shape we know how to map
            }
            return Some(format!("https://{rest}:8080"));
        }
        base.replace(":1984", ":8788").into()
    }

    /// Grab the warm frame from the edge's own `/snapshot/:ip`. Short timeout: this is a fast path,
    /// and when it isn't available we must fall back quickly rather than stall the workflow node.
    fn warm_snapshot(&self, ip: &str) -> Result<Vec<u8>, String> {
        // SSE first: the box is outbound-only, so this is the ONLY route that reaches it.
        if let Some(res) = self.sse_snapshot(ip) {
            return res;
        }
        let base = self.edge_api_base().ok_or("no edge api base")?;
        if edge_api_dead()
            .lock()
            .map(|d| d.get(&base).is_some_and(|at| at.elapsed() < DEAD_FOR))
            .unwrap_or(false)
        {
            return Err("edge api unreachable (retrying shortly)".into());
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(2))
            .build();
        let url = format!("{base}/snapshot/{}", ip);
        let res = (|| -> Result<Vec<u8>, String> {
            let resp = agent
                .get(&url)
                .call()
                .map_err(|e| format!("edge snapshot: {e}"))?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes)
                .map_err(|e| format!("reading edge snapshot: {e}"))?;
            if bytes.is_empty() {
                return Err("edge returned an empty frame".into());
            }
            Ok(bytes)
        })();
        if res.is_err() {
            if let Ok(mut d) = edge_api_dead().lock() {
                d.insert(base, std::time::Instant::now());
            }
        }
        res
    }
}

impl FrameSource for EdgeFrameSource {
    /// fallback to a sidecar that is never started can only add a connection-refused and its
    /// timeout to every miss.
    fn snapshot(&self, device: &Device) -> Result<Vec<u8>, String> {
        self.warm_snapshot(&device.ip)
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
    fn detect_objects_detailed(
        &self,
        jpeg: &[u8],
        class_id: u32,
    ) -> Result<Vec<PersonDetection>, String> {
        crate::agent::detector::detect_objects_detailed(jpeg, class_id)
    }
    fn describe(&self, jpeg: &[u8], label: &str) -> Result<String, String> {
        crate::agent::vision::describe_image(jpeg, label)
    }
    fn ask_yes_no(&self, question: &str, image: Option<&[u8]>) -> Result<bool, String> {
        crate::agent::vision::ask_yes_no(question, image)
    }
    fn ask_text(&self, prompt: &str, image: Option<&[u8]>) -> Result<String, String> {
        crate::agent::vision::ask_text(prompt, image)
    }
    fn classify(&self, text: &str, arms: &[String]) -> Result<Option<String>, String> {
        crate::agent::vision::classify_intent(text, arms)
    }
    fn extract(&self, text: &str, field: &str) -> Result<String, String> {
        crate::agent::vision::extract_field(text, field)
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

    fn reply(
        &self,
        cfg: &NotificationConfig,
        chat_id: i64,
        message: &str,
        image: Option<Vec<u8>>,
    ) -> Result<String, String> {
        match image {
            Some(jpeg) => crate::notification::client::send_photo(cfg, chat_id, message, &jpeg)?,
            None => crate::notification::client::send_message(cfg, chat_id, message)?,
        }
        Ok("sent".to_string())
    }

    fn reply_video(
        &self,
        cfg: &NotificationConfig,
        chat_id: i64,
        caption: &str,
        video: Vec<u8>,
    ) -> Result<String, String> {
        crate::notification::client::send_video(cfg, chat_id, caption, &video)?;
        Ok("sent".to_string())
    }
}

/// Notifications via the durable queue — flow only ENQUEUEs; `sentinel-notification` consumes
/// and sends (card #57). Baked with the run's `db` + `tenant_id` because the `Notifier` trait
/// doesn't carry them. An `image` is uploaded to the object store and its key rides the queue
/// row; the consumer downloads + sends. If the object store isn't configured (or upload fails),
/// the image is delivered in-process so it's never lost.
pub struct RemoteNotifier {
    pub db: crate::db::Db,
    pub tenant_id: uuid::Uuid,
}
impl Notifier for RemoteNotifier {
    fn dispatch(
        &self,
        store: &NotificationStore,
        cfg: &NotificationConfig,
        target: Option<&str>,
        message: &str,
        image: Option<Vec<u8>>,
    ) -> Result<String, String> {
        let image_key = match image {
            Some(bytes) if crate::media::enabled() => {
                match crate::media::put(&bytes, "image/jpeg", "jpg") {
                    Ok(key) => Some(key),
                    Err(e) => {
                        log::warn!("media upload failed ({e}); delivering image in-process");
                        return crate::notification::dispatch(
                            store,
                            cfg,
                            target,
                            message,
                            Some(bytes),
                        );
                    }
                }
            }
            Some(bytes) => {
                return crate::notification::dispatch(store, cfg, target, message, Some(bytes))
            }
            None => None,
        };
        crate::work_queue::enqueue_notification_blocking(
            &self.db,
            self.tenant_id,
            target,
            None, // beacon-targeted send (not a direct chat reply — see reply())
            message,
            image_key.as_deref(),
            None,
        );
        Ok("queued".to_string())
    }

    fn reply(
        &self,
        cfg: &NotificationConfig,
        chat_id: i64,
        message: &str,
        image: Option<Vec<u8>>,
    ) -> Result<String, String> {
        // Upload the image to the object store; the consumer downloads + sends. If the store is
        // off or the upload fails, send the reply in-process so it's never lost.
        let image_key = match image {
            Some(bytes) if crate::media::enabled() => {
                match crate::media::put(&bytes, "image/jpeg", "jpg") {
                    Ok(key) => Some(key),
                    Err(e) => {
                        log::warn!("media upload failed ({e}); replying with image in-process");
                        crate::notification::client::send_photo(cfg, chat_id, message, &bytes)?;
                        return Ok("sent".to_string());
                    }
                }
            }
            Some(bytes) => {
                crate::notification::client::send_photo(cfg, chat_id, message, &bytes)?;
                return Ok("sent".to_string());
            }
            None => None,
        };
        crate::work_queue::enqueue_notification_blocking(
            &self.db,
            self.tenant_id,
            None,
            Some(chat_id),
            message,
            image_key.as_deref(),
            None,
        );
        Ok("queued".to_string())
    }

    fn reply_video(
        &self,
        cfg: &NotificationConfig,
        chat_id: i64,
        caption: &str,
        video: Vec<u8>,
    ) -> Result<String, String> {
        // Video is heavy → always through the object store + queue. In-process only if media is off.
        if !crate::media::enabled() {
            crate::notification::client::send_video(cfg, chat_id, caption, &video)?;
            return Ok("sent".to_string());
        }
        let key = crate::media::put(&video, "video/mp4", "mp4")?;
        crate::work_queue::enqueue_notification_blocking(
            &self.db,
            self.tenant_id,
            None,
            Some(chat_id),
            caption,
            None,
            Some(&key),
        );
        Ok("queued".to_string())
    }
}

#[cfg(test)]
mod edge_api_base_tests {
    use super::EdgeFrameSource;

    /// (`tailscale serve --https=8080 8788`).
    #[test]
    fn tailnet_https_base_maps_to_8080() {
        let s = EdgeFrameSource::new("https://box-abc.tail1234.ts.net");
        assert_eq!(
            s.edge_api_base().as_deref(),
            Some("https://box-abc.tail1234.ts.net:8080")
        );
    }

    /// A trailing slash must not produce a double slash in the derived URL.
    #[test]
    fn trailing_slash_is_trimmed() {
        let s = EdgeFrameSource::new("https://box.ts.net/");
        assert_eq!(
            s.edge_api_base().as_deref(),
            Some("https://box.ts.net:8080")
        );
    }

    /// A legacy local base (the old sidecar port) still maps to the edge's own HTTP port.
    #[test]
    fn legacy_localhost_base_maps_to_edge_http() {
        let s = EdgeFrameSource::new("http://127.0.0.1:1984");
        assert_eq!(s.edge_api_base().as_deref(), Some("http://127.0.0.1:8788"));
    }

    /// An https base that already carries an explicit port isn't a shape we can map — better to
    #[test]
    fn explicit_https_port_declines() {
        let s = EdgeFrameSource::new("https://box.ts.net:9443");
        assert_eq!(s.edge_api_base(), None);
    }
}
