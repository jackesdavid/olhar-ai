//! Persistent per-camera frame decoder (#120). One **supervised**, long-lived decoder process reads
//! the frame hub reads the freshest decoded frame with **no per-request `frame.jpeg` cold-start**
//!
//! (default) or `gstreamer`, picked per box via `RecordingConfig.media_backend`. Both emit the SAME
//! wire format — concatenated JPEG images on stdout — so [`read_mjpeg`] doesn't care which produced
//! them. This is a coexistence/measurement phase, not a permanent fork; see the plan doc.
//!
//! **Resilient by design:** the decoder exits (stream drop) OR wedges (alive but no frames) are both
//! detected — a watchdog kills a stalled process so the read unblocks, and the supervisor respawns
//! frame ([`latest_fresh`] returns `None`), so capture never stops even while a decoder is restarting.

//! ---------------------------------------------------------------------------------------------
//! ## INVARIANT: ONE GStreamer pipeline per camera. All consumers hang off `tee` branches.
//!
//! **Do not open a second connection to a camera. Ever.** Not for live, not for a snapshot, not for
//! recording, not for motion, not for face capture. The edge dials each camera exactly ONCE and
//! every consumer takes a `tee` branch off that single pipeline.
//!
//! Why this is a hard rule and not a preference:
//!
//! - **The camera is the scarce resource.** Consumer IP cameras cap concurrent RTSP sessions at 2–4.
//!   A second dial for a snapshot is what produced "503 Service Unavailable — insufficient
//!   resource" and made a previously-working camera go dark (see `media.rs`). The failure lands on
//!   the CUSTOMER's camera, not on us, and it looks like their hardware broke.
//! - **Cost is per-connection, not per-consumer.** Decode is the expensive part; a tee branch is
//!   nearly free. Two pipelines is 2x CPU and 2x network for the same picture.
//! - **This box is not a server.** It runs on a Mac mini, a Jetson, and a phone. Head-room is the
//!   feature.
//!
//! If you need frames somewhere new, add a branch — `crate::capture::mse_feed`-style — and never a
//! new `RtspSource`. If a branch needs a different resolution or codec, transform it after the tee;
//! do not re-dial.
//! ---------------------------------------------------------------------------------------------
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Cam {
    latest: Mutex<Option<(Arc<Vec<u8>>, Instant)>>, // freshest JPEG + when it arrived
    started: AtomicBool,                            // supervisor spawned?
    /// Live fMP4 for the `/api/ws` viewers, muxed on a `tee` branch of this camera's ONE pipeline
    /// (card #138, G2) — not a second connection to the camera.
    mse: Mutex<crate::gst_mse::Feed>,
    /// The codec detected by [`probe_codec`] on the gstreamer-linked path ("H264"/"H265") —
    /// `None` until the pipeline's first (re)start has probed it. Surfaced to the cloud so the
    /// Devices page can show what each camera is actually sending.
    codec: Mutex<Option<&'static str>>,
}

fn cams() -> &'static Mutex<HashMap<String, Arc<Cam>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<Cam>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Slot key for a camera stream. Split keeps preview (`ip`) and HD (`ip/hd`) independent so motion
/// can stay on the substream while Live HD dials main. Unified collapses everything onto the
/// preview slot (which already dials main via [`crate::state::pick_camera_rtsp`]).
fn cam_key(ip: &str, hd: bool) -> String {
    if hd {
        format!("{ip}/hd")
    } else {
        ip.to_string()
    }
}

/// URL scheme for a camera whose frames arrive from the HOST APPLICATION instead of over RTSP —
/// the phone's own front/back camera (#175 Phase 1).
///
/// It is a URL rather than a flag so the existing plumbing carries it untouched: `frames::spawn`
/// only starts a hub for a device with a non-empty `rtsp_url`, and a device camera genuinely has a
/// source, just not a dialable one. Everything downstream of the frame — motion, YuNet, the quality
/// signals, the events — is byte-identical to a wall camera's, which is the entire point of feeding
/// the same slot rather than adding a parallel path.
pub const DEVICE_SCHEME: &str = "device:";

/// Is this a host-app camera rather than something to dial?
pub fn is_device_url(url: &str) -> bool {
    url.trim_start().starts_with(DEVICE_SCHEME)
}

/// Is `ip` a camera the host app feeds?
pub fn is_device_camera(state: &crate::state::AppState, ip: &str) -> bool {
    state
        .device_by_ip(ip)
        .and_then(|d| d.rtsp_url.clone())
        .map(|u| is_device_url(&u))
        .unwrap_or(false)
}

/// Publish a frame captured by the host app into `ip`'s preview slot (#175 Phase 1).
///
/// This is the ONLY writer for a device camera, and it marks the slot `started` so no supervisor
/// ever tries to dial it — the ONE-PIPELINE invariant above is about not opening a second
/// connection to a camera, and here there is no connection at all.
///
/// `jpeg` is what the phone's encoder produced. The hub decodes it exactly once, like any other
/// source, so sharpness and pose are measured on the same kind of buffer the wall camera's are —
/// which is what makes a phone-vs-camera comparison mean anything.
pub fn push_device_frame(ip: &str, jpeg: Vec<u8>) {
    let at = Instant::now();
    // Recording first, and only while a ring is actually running (#175). A device camera has no
    // H264 of its own, so its ring is fed by re-encoding these JPEGs; with no ring there is no
    // encoder and this is a map lookup, so a phone that is not recording pays nothing.
    crate::recording::feed_device_frame(ip, &jpeg, at);

    let cam = cam_for(ip, false);
    cam.started.store(true, Ordering::SeqCst);
    *cam.latest.lock().unwrap() = Some((Arc::new(jpeg), at));
}

/// Which decoder slot a request should join. Unified never opens a second main-stream session.
pub fn stream_slot(state: &crate::state::AppState, ip: &str, hd: bool) -> bool {
    // A device camera has ONE stream — no main/sub pair to choose between — so it always uses the
    // preview slot, which is where `push_device_frame` writes. Without this, a box whose
    // `face_capture_stream` is "hd" would have the hub read `ip/hd` while the app filled `ip`, and
    // the camera would look permanently frozen with no error anywhere.
    if is_device_camera(state, ip) {
        return false;
    }
    let unified = state
        .device_by_ip(ip)
        .map(|d| d.is_unified())
        .unwrap_or(false);
    hd && !unified
}

fn cam_for(ip: &str, hd: bool) -> Arc<Cam> {
    let mut map = cams().lock().unwrap();
    map.entry(cam_key(ip, hd))
        .or_insert_with(|| {
            Arc::new(Cam {
                latest: Mutex::new(None),
                started: AtomicBool::new(false),
                mse: Mutex::new(crate::gst_mse::Feed::default()),
                codec: Mutex::new(None),
            })
        })
        .clone()
}


/// Ensure a supervised decoder is running for `ip` on the requested stream. Idempotent **per
/// slot** — Split mode can run preview (sub) and HD (main) at once; Unified keeps a single
/// "gstreamer", card #138).
pub fn ensure(state: &crate::state::AppState, ip: &str, hd: bool) {
    let slot = stream_slot(state, ip, hd);
    let cam = cam_for(ip, slot);
    if cam.started.swap(true, Ordering::SeqCst) {
        return;
    }
    // A device camera has nothing to dial — the host app pushes into the slot via
    // `push_device_frame`. Returning here (rather than spawning a supervisor that would fail to
    // parse `device:front` forever) is what keeps the retry log quiet and the CPU idle.
    if is_device_camera(state, ip) {
        log::info!("capture[{ip}]: device camera — frames pushed by the host app, no pipeline");
        return;
    }
    // We dial the camera ourselves — that is the difference between owning the ingest and renting
    // it from a sidecar. `slot` (not the caller's raw `hd`) picks the URL: unified always resolves
    // to main inside `camera_rtsp` via `Device::is_unified`, even when the slot is the preview key.
    let url = state.camera_rtsp(ip, slot);
    ensure_started(ip, slot, cam, url);
}

/// Start the supervisor for a camera whose RTSP URL the caller already has — the same path as
/// [`ensure`], minus the `AppState` lookup, so it can be driven from a test.
fn ensure_started(ip: &str, hd: bool, cam: Arc<Cam>, url: Option<String>) {
    let ip = ip.to_string();
    std::thread::spawn(move || supervise(&ip, hd, cam, url));
}

/// Test-only entry point: dial `url` directly for `ip`, bypassing device state.
#[cfg(test)]
fn ensure_with_url(ip: &str, hd: bool, url: &str) {
    let cam = cam_for(ip, hd);
    if cam.started.swap(true, Ordering::SeqCst) {
        return;
    }
    ensure_started(ip, hd, cam, Some(url.to_string()));
}

/// What a live viewer needs for `ip` on the given stream slot: the MSE init segment, the MIME for
/// `addSourceBuffer`, and the live fragments from here on. `None` until that slot's pipeline has
/// muxed its first fragment. `hd` must match the slot [`ensure`] joined (use [`stream_slot`]).
pub fn mse_feed(
    ip: &str,
    hd: bool,
) -> Option<(
    Vec<u8>,
    String,
    tokio::sync::broadcast::Receiver<Arc<Vec<u8>>>,
)> {
    let cam = cams().lock().unwrap().get(&cam_key(ip, hd)).cloned()?;
    let feed = cam.mse.lock().unwrap().subscribe();
    feed
}

/// The video codec [`probe_codec`] detected for `ip` ("H264"/"H265"), if a gstreamer-linked
/// pipeline has started and probed it at least once. Prefers the HD slot when both are running
/// (Devices page cares about the camera's main encode).
pub fn codec_for(ip: &str) -> Option<&'static str> {
    for hd in [true, false] {
        if let Some(cam) = cams().lock().unwrap().get(&cam_key(ip, hd)).cloned() {
            if let Some(c) = *cam.codec.lock().unwrap() {
                return Some(c);
            }
        }
    }
    None
}

/// The freshest decoded JPEG for `ip` on the given stream slot, only if newer than `max_age` —
/// How long since a frame last arrived for `ip`, or `None` if one never has.
///
/// This — not the RTSP probe — is what distinguishes "Streaming" from merely "Reachable". A camera
/// can pass DESCRIBE and still deliver nothing (a wedged encoder, a pipeline that never negotiated),
/// and only the frame hub can tell.
pub fn last_frame_age(ip: &str) -> Option<Duration> {
    let cam = cams().lock().unwrap().get(&cam_key(ip, false)).cloned()?;
    let g = cam.latest.lock().unwrap();
    g.as_ref().map(|(_, at)| at.elapsed())
}

pub fn latest_fresh(ip: &str, hd: bool, max_age: Duration) -> Option<Arc<Vec<u8>>> {
    let cam = cams().lock().unwrap().get(&cam_key(ip, hd)).cloned()?;
    let g = cam.latest.lock().unwrap();
    match &*g {
        Some((jpeg, at)) if at.elapsed() <= max_age => Some(Arc::clone(jpeg)),
        _ => None,
    }
}

fn stream_url(ip: &str, hd: bool) -> String {
    let name = if hd {
        app_core::stream_names::stream_name_hd(ip)
    } else {
        app_core::stream_names::stream_name(ip)
    };
    format!("rtsp://127.0.0.1:8554/{name}")
}

/// **in-process** (no subprocess), publishing each JPEG to `cam.latest` via `appsink`, keeping the
/// exact MJPEG output contract so every downstream consumer (snapshot, detector, hub) is unchanged.
/// `decodebin` autoplugs the platform HW decoder for a real camera stream (`vtdec` on Mac); on the
/// Jetson the `nvv4l2decoder → NVMM` memory needs `nvvidconv` in place of `videoconvert` — that
/// swap is the Jetson gate's job. Blocks until the pipeline errors, then returns so `supervise()`
fn run_gst_linked(ip: &str, hd: bool, cam: &Arc<Cam>, url: Option<&str>) -> Result<(), String> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;

    crate::media::init_gstreamer()?; // init + iOS static plugin registration
                                     // RTSP never resolved. Owning the dial is what lets the sidecar go away.
    let url = url
        .map(str::to_string)
        .unwrap_or_else(|| stream_url(ip, hd));

    // ONE ingest, every platform: `rtspsrc` dials the camera directly.
    //
    // There used to be a Tegra-only fork here that pulled with `retina` and fed `appsrc`, because
    // GStreamer 1.20's `rtspsrc` stalls at SETUP on some cameras' Digest auth (verified against a
    // live Dahua on an Orin Nano). It is gone: two ingest paths meant two sets of timing, caps and
    // failure behaviour to reason about, and the platform that got the LESS-tested path was the one
    // hardest to debug. Jetson is expected to run this same pipeline; making it do so is tracked
    // separately rather than papered over with a second code path.

    // PROBE the camera's codec from its SDP and build a matching pipeline — H264 or H265 — on
    // EVERY platform, not just Tegra/retina: cameras vary (a Dahua that was H265 vs one switched
    // to H264) regardless of which platform is watching them, and only the depay/parser/caps
    // differ; `decodebin`/`nvv4l2decoder` decode both. Used both to pick the right elements below
    // and to report the camera's codec (`codec_for`) for the UI.
    let codec = match probe_codec(&url) {
        Ok(c) => {
            log::info!("capture-decoder[{ip}]: codec probe → {}", c.label());
            c
        }
        Err(e) => return Err(format!("codec probe: {e}")),
    };
    *cam.codec.lock().unwrap() = Some(codec.label());

    // ONE connection, `tee`'d: the decode branch feeds motion + detectors, the mux branch feeds the
    // live `/api/ws` viewers. Both leak-on-full — a consumer that stalls must never back the source
    // up and take the other branch down with it.
    //
    // The mux branch re-parses to a muxer-friendly stream-format: a `tee` forces one output format on
    // the shared parser, and the two branches disagree — the decoder takes byte-stream, while `mp4mux`
    // on GStreamer 1.20 wants avc/hvc1 and answers a byte-stream query with EMPTY caps. The pipeline
    // then dies at the source with `not-negotiated`, naming neither the element nor the reason.
    let mux = crate::gst_mse::FRAGMENT_MS;
    // Picked from this box's registry, not from a platform guess — see `media::decode_branch`.
    let decode = crate::media::decode_branch();
    let parse = codec.parse_element(); // h264parse / h265parse
    let mux_caps = codec.mux_caps(); // avc / hvc1
    let source = format!(
        "rtspsrc location=\"{url}\" protocols=tcp latency=200 ! {}",
        codec.depay_element()
    );
    let desc = format!(
        "{source} ! {parse} name=parse ! tee name=t \
         t. ! queue max-size-buffers=64 leaky=downstream ! {decode} ! videorate \
            ! video/x-raw,framerate=6/1 ! jpegenc quality=85 \
            ! appsink name=out sync=false max-buffers=2 drop=true \
         t. ! queue max-size-buffers=64 leaky=downstream \
            ! {parse} ! {mux_caps} \
            ! mp4mux fragment-duration={mux} streamable=true \
            ! appsink name=mse sync=false"
    );
    let pipeline = gst::parse::launch(&desc)
        .map_err(|e| format!("parse: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "not a pipeline".to_string())?;
    let appsink = pipeline
        .by_name("out")
        .ok_or("appsink missing")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| "not an appsink".to_string())?;

    let cam_cb = Arc::clone(cam);
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                if let Some(buf) = sample.buffer() {
                    if let Ok(map) = buf.map_readable() {
                        // Same contract as read_mjpeg: freshest JPEG becomes `cam.latest`.
                        *cam_cb.latest.lock().unwrap() =
                            Some((Arc::new(map.to_vec()), Instant::now()));
                    }
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    // PTS → DTS. `mp4mux` on the Jetson's GStreamer 1.20 refuses a stream whose DTS is unset
    // ("Could not multiplex stream", with `dts: 99:99:99.999999999` in its debug log); the Mac's
    // 1.28 tolerates it, which is exactly how this got missed. RTP depayloading supplies only PTS,
    // and our cameras carry no B-frames, so copying one to the other is both correct and enough.
    if let Some(parse_src) = pipeline.by_name("parse").and_then(|e| e.static_pad("src")) {
        parse_src.add_probe(gst::PadProbeType::BUFFER, |_, info| {
            if let Some(gst::PadProbeData::Buffer(buffer)) = &mut info.data {
                if buffer.dts().is_none() {
                    let pts = buffer.pts();
                    buffer.make_mut().set_dts(pts);
                }
            }
            gst::PadProbeReturn::Ok
        });
    }

    let mse_sink = pipeline
        .by_name("mse")
        .ok_or("mse appsink missing")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| "mse sink is not an appsink".to_string())?;
    let cam_mse = Arc::clone(cam);
    mse_sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                if let Some(buf) = sample.buffer() {
                    if let Ok(map) = buf.map_readable() {
                        cam_mse.mse.lock().unwrap().on_box(map.as_slice());
                    }
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );


    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| format!("play: {e}"))?;
    // Wait on the bus for EOS/error, WITH a frame-freshness watchdog. A live source normally only
    // ends on failure — but a pipeline can also wedge in PLAYING and post nothing (a decoder that
    // never negotiates, or a source that connects but stops sending). `iter_timed(NONE)` would block
    // forever; poll on a short timeout and bail if no JPEG has landed within the grace window, so
    // `supervise` tears down (which also stops the feeder) and restarts under its backoff policy.
    let bus = pipeline.bus().ok_or("no bus")?;
    let started = Instant::now();
    let mut result = Ok(());
    loop {
        let last_frame = cam.latest.lock().unwrap().as_ref().map(|(_, at)| *at);
        let stalled = is_stalled(last_frame, started, Instant::now());
        if stalled {
            result = Err("no frames within watchdog window — restarting".to_string());
            break;
        }
        match bus.timed_pop(gst::ClockTime::from_seconds(2)) {
            Some(msg) => match msg.view() {
                gst::MessageView::Eos(_) => break,
                gst::MessageView::Error(e) => {
                    result = Err(format!("{}", e.error()));
                    break;
                }
                _ => {}
            },
            None => {} // 2s tick with no message → re-check the watchdog
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    result
}

/// Has THIS pipeline run gone too long without a frame?
///
/// `last_frame` is `cam.latest`'s timestamp, which lives on the shared `Cam` and is deliberately
/// **never cleared** — viewers keep seeing the last image while a decoder restarts (see the module
/// docs). That made it the wrong clock to judge a fresh pipeline by, and the bug was severe:
///
/// The old test was `at.elapsed() > 12s` for any `Some`. Once a camera had ever delivered a frame,
/// `latest` stayed `Some` forever, so after a stall longer than 12s EVERY subsequent run was
/// declared stalled on its first loop iteration — before the new pipeline could produce anything.
/// The 20s warm-up grace was reachable only by a camera that had never worked at all. The
/// supervisor then restarted, the new run died instantly, and the camera stayed dark: the recovery
/// mechanism was what prevented recovery. Worse, each restart dials a fresh RTSP session, and a
/// consumer camera caps concurrent sessions at 2–4 (the ONE-PIPELINE invariant at the top of this
/// module) — so the restart storm exhausts the very slots it needs to recover through.
///
/// A frame older than this run therefore does not count: the run gets the same warm-up grace as a
/// camera being seen for the first time.
fn is_stalled(last_frame: Option<Instant>, run_started: Instant, now: Instant) -> bool {
    /// No frame for this long **within a run** → the pipeline is wedged in PLAYING.
    const STALL_AFTER: Duration = Duration::from_secs(12);
    /// How long a fresh pipeline may take to produce its first frame before we give up on it.
    const WARMUP: Duration = Duration::from_secs(20);

    match last_frame {
        Some(at) if at >= run_started => now.duration_since(at) > STALL_AFTER,
        // Either no frame ever, or the newest one predates this run — same thing here.
        _ => now.duration_since(run_started) > WARMUP,
    }
}

/// The video codec a camera streams. Only the parser + caps differ in the graph — `nvv4l2decoder`
/// and `mp4mux` handle both. Detected from the SDP ([`probe_codec`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum VideoCodec {
    H264,
    H265,
}

impl VideoCodec {
    fn from_encoding(enc: &str) -> Option<Self> {
        match enc.to_ascii_lowercase().as_str() {
            "h264" => Some(Self::H264),
            "h265" | "hevc" => Some(Self::H265),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::H264 => "H264",
            Self::H265 => "H265",
        }
    }
    /// The parser element — shared by the decode and mux branches.
    fn parse_element(self) -> &'static str {
        match self {
            Self::H264 => "h264parse",
            Self::H265 => "h265parse",
        }
    }
    /// The RTP depayloader for the `rtspsrc` ingest path (non-Tegra platforms).
    fn depay_element(self) -> &'static str {
        match self {
            Self::H264 => "rtph264depay",
            Self::H265 => "rtph265depay",
        }
    }
    /// The mux-branch caps `mp4mux` accepts (avc for H264, hvc1 for H265).
    fn mux_caps(self) -> &'static str {
        match self {
            Self::H264 => "video/x-h264,stream-format=avc,alignment=au",
            Self::H265 => "video/x-h265,stream-format=hvc1,alignment=au",
        }
    }
    fn caps_media(self) -> &'static str {
        match self {
            Self::H264 => "video/x-h264",
            Self::H265 => "video/x-h265",
        }
    }
    /// The appsrc `stream-format` for retina's codec-framed access units.
    fn stream_format(self) -> &'static str {
        match self {
            Self::H264 => "avc",
            Self::H265 => "hvc1",
        }
    }
}

/// Probe a camera's video codec from its RTSP SDP (a `retina` DESCRIBE, no PLAY) so the pipeline can
/// be built to match. Cheap; runs once per capture (re)start — a future card caches this per camera
/// and lets the UI override it.
fn probe_codec(url: &str) -> Result<VideoCodec, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio rt: {e}"))?;
    rt.block_on(async {
        let (bare, creds) = crate::recording::split_rtsp_credentials(url);
        let parsed = bare
            .parse::<url::Url>()
            .map_err(|e| format!("bad url: {e}"))?;
        let opts = match creds {
            Some((username, password)) => retina::client::SessionOptions::default()
                .creds(Some(retina::client::Credentials { username, password })),
            None => retina::client::SessionOptions::default(),
        };
        let session = retina::client::Session::describe(parsed, opts)
            .await
            .map_err(|e| format!("describe: {e}"))?;
        session
            .streams()
            .iter()
            .filter(|s| s.media() == "video")
            .find_map(|s| VideoCodec::from_encoding(s.encoding_name()))
            .ok_or_else(|| "no H264/H265 video stream in SDP".to_string())
    })
}



fn supervise(ip: &str, hd: bool, cam: Arc<Cam>, url: Option<String>) {
    log::info!(
        "capture-decoder[{ip}]: starting (stream={})",
        if hd { "HD (main)" } else { "preview (sub)" }
    );
    let mut backoff = Duration::from_millis(500);
    let mut fails: u32 = 0;
    loop {
        let ran_from = Instant::now();

        // ONE in-process GStreamer pipeline per camera (see the module invariant). There is no
        // GStreamer picks its own decoder in `media::decode_branch()`.
        if let Err(e) = run_gst_linked(ip, hd, &cam, url.as_deref()) {
            log::warn!("capture-decoder[{ip}]: {e}");
        }

        // Reset the backoff ONLY after a healthy run. A fast-dying pipeline must not reset it —
        // that was the tight-loop storm. Repeated fast failures cool down hard.
        if ran_from.elapsed() >= Duration::from_secs(30) {
            if fails > 0 {
                log::info!("capture-decoder[{ip}]: recovered — streaming again");
            }
            backoff = Duration::from_millis(500);
            fails = 0;
        } else {
            fails += 1;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            if fails == 5 {
                log::warn!(
                    "capture-decoder[{ip}]: repeated failures — backing off to 30s"
                );
            }
        }
        std::thread::sleep(backoff);
    }
}

/// frame as the camera's latest. Returns on EOF/error (→ supervisor respawns).
fn read_mjpeg(mut stdout: std::process::ChildStdout, cam: &Arc<Cam>) {
    let mut buf: Vec<u8> = Vec::with_capacity(256 * 1024);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        // Extract every complete JPEG (SOI…EOI), keeping the freshest as `latest`.
        loop {
            // Align the buffer to the next SOI, dropping any junk/partial before it.
            match find(&buf, &[0xFF, 0xD8]) {
                Some(0) => {}
                Some(soi) => drop(buf.drain(..soi)),
                None => {
                    buf.clear(); // no frame start in view — reset
                    break;
                }
            }
            if buf.len() < 4 {
                break; // need more bytes before we can find an EOI
            }
            match find(&buf[2..], &[0xFF, 0xD9]) {
                Some(rel) => {
                    let end = 2 + rel + 2; // past the leading SOI + the 2-byte EOI marker
                    let jpeg = buf[..end].to_vec();
                    buf.drain(..end);
                    *cam.latest.lock().unwrap() = Some((Arc::new(jpeg), Instant::now()));
                }
                None => break, // EOI not arrived yet — wait for more data
            }
        }
        if buf.len() > 8 * 1024 * 1024 {
            buf.clear(); // corrupt / no EOI — don't grow unbounded
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// `is_keyframe_tests` lived here and is gone. It covered `is_keyframe`, a function deleted when
// GStreamer took over depacketizing — `h264parse`/`isomp4` decide keyframes now, so there is no
// longer anything of ours to test. The module survived the deletion only because it was gated on
// `feature = "gst-linked"`, itself deleted in 277f079: it had not compiled since, and restoring the
// gate would have meant re-implementing a function nothing calls.
//
// Two of these were left by the same commit (see the media_smoke doc below for the other). A cfg
// referencing a feature that does not exist removes the code silently — `cargo check` reports
// nothing, and a dead test reads exactly like a passing one.

#[cfg(test)]
mod media_smoke {
    //! Card #159 — the media path, end to end, against a synthetic RTSP source.
    //!
    //! `check:rust` proves the pipeline COMPILES. Nothing proved a camera still produces frames,
    //! and that gap is not theoretical: the capture watchdog shipped a bug (#164) that killed every
    //! pipeline restart, reached a customer box, survived several releases, and surfaced only when
    //! a human noticed their camera was dark.
    //!
    //! This replaces a test that was worse than nothing. It was gated on `feature = "gst-linked"`,
    //! a feature deleted in 277f079, and called `ensure_with_url` with four arguments when it takes
    //! three — so it had never compiled, let alone run, while reading like coverage.
    //!
    //! Runs only when `MEDIA_SMOKE_RTSP` names a reachable RTSP URL; `check:media` in
    //! .gitlab-ci.yml sets it to a mediamtx-served test pattern. Unset, it SKIPS LOUDLY — and the
    //! CI job greps for the ran-marker, so a skip cannot masquerade as a pass.
    use std::collections::HashMap;
    use std::time::Duration;

    const RAN_MARKER: &str = "media_smoke: RAN";

    #[test]
    fn one_connection_yields_frames_live_fragments_and_a_clip() {
        let Ok(url) = std::env::var("MEDIA_SMOKE_RTSP") else {
            eprintln!("media_smoke: SKIPPED — set MEDIA_SMOKE_RTSP to an RTSP URL to run it.");
            return;
        };
        // A key, not a dial: the URL above is what actually gets connected.
        let ip = "10.255.255.1";
        super::ensure_with_url(ip, false, &url);

        // ---- 1. frames out ---------------------------------------------------------------
        // This is also THE SNAPSHOT PATH: `GET /snapshot/:ip` is `capture::ensure` +
        // `capture::latest_fresh`, so a fresh JPEG here is the snapshot working.
        let mut jpeg = None;
        for _ in 0..60 {
            if let Some(j) = super::latest_fresh(ip, false, Duration::from_secs(5)) {
                jpeg = Some(j);
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let jpeg = jpeg.expect("decode branch produced no JPEG within 30s");
        assert!(
            jpeg.starts_with(&[0xFF, 0xD8]),
            "not a JPEG: {:02x?}",
            &jpeg[..4.min(jpeg.len())]
        );
        assert!(jpeg.len() > 1024, "JPEG suspiciously small: {} B", jpeg.len());

        // ---- 2. the live mux branch, off the SAME connection ------------------------------
        let (init, mime, mut rx) = super::mse_feed(ip, false).expect("mux branch produced no feed");
        assert_eq!(&init[4..8], b"ftyp", "init segment must open with ftyp");
        assert!(
            init.windows(4).any(|w| w == b"moov"),
            "init segment must carry moov"
        );
        assert!(
            mime.starts_with("video/mp4; codecs=\"avc1."),
            "unexpected mime: {mime}"
        );
        for _ in 0..3 {
            rx.blocking_recv().expect("fragment channel closed");
        }

        // ---- 3. a clip is muxed to a playable file ----------------------------------------
        let dir = std::env::temp_dir().join("media-smoke-clips");
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("EDGE_RECORDING_DIR", &dir);
        // Unreachable control on purpose: `boot` falls back to RecordingConfig::default().
        let mgr = crate::recording::RecordingManager::boot("http://127.0.0.1:1");
        mgr.set_dials(HashMap::from([(ip.to_string(), url.clone())]));
        let (clip_id, started) = mgr.start(ip, 0).expect("recording did not start");
        assert!(started, "expected a NEW recording session");
        std::thread::sleep(Duration::from_secs(4));
        let meta = mgr.stop(&clip_id).expect("recording did not stop cleanly");

        let mp4 = dir.join(format!("{clip_id}.mp4"));
        let bytes = std::fs::read(&mp4).unwrap_or_else(|e| panic!("clip {mp4:?} unreadable: {e}"));
        assert!(
            bytes.len() > 8 * 1024,
            "clip is {} B — a header with no video is not a recording",
            bytes.len()
        );
        assert_eq!(&bytes[4..8], b"ftyp", "clip must open with ftyp");
        assert!(
            bytes.windows(4).any(|w| w == b"moov"),
            "clip must carry moov — an unfinalised mp4 will not play"
        );
        assert!(meta.size_bytes > 0, "clip metadata reports zero bytes");

        let _ = std::fs::remove_file(&mp4);
        // The CI job greps for this: a silent skip must never read as a pass.
        println!("{RAN_MARKER} frames={} B, clip={} B", jpeg.len(), bytes.len());
    }
}

#[cfg(test)]
mod watchdog_tests {
    use super::is_stalled;
    use std::time::{Duration, Instant};

    /// The bug (2026-08-02): `cam.latest` is never cleared, so after a stall the NEXT run saw a
    /// frame from the previous session, called itself stalled on its first check, and died before
    /// it could produce anything. The camera then never came back on its own.
    #[test]
    fn a_frame_from_the_previous_run_does_not_condemn_a_fresh_pipeline() {
        let now = Instant::now();
        let run_started = now - Duration::from_secs(1);
        let last_frame = now - Duration::from_secs(600); // 10 min old, previous session
        assert!(
            !is_stalled(Some(last_frame), run_started, now),
            "a new pipeline must get its warm-up grace, not inherit the old run's staleness"
        );
    }

    #[test]
    fn a_fresh_pipeline_still_gives_up_if_it_never_produces() {
        let now = Instant::now();
        let run_started = now - Duration::from_secs(21);
        let last_frame = now - Duration::from_secs(600);
        assert!(
            is_stalled(Some(last_frame), run_started, now),
            "past the warm-up window with nothing from this run, it is wedged"
        );
    }

    #[test]
    fn a_running_pipeline_that_goes_quiet_is_stalled() {
        let now = Instant::now();
        let run_started = now - Duration::from_secs(300);
        let last_frame = now - Duration::from_secs(13);
        assert!(is_stalled(Some(last_frame), run_started, now));
    }

    #[test]
    fn a_healthy_pipeline_is_not_stalled() {
        let now = Instant::now();
        let run_started = now - Duration::from_secs(300);
        let last_frame = now - Duration::from_secs(2);
        assert!(!is_stalled(Some(last_frame), run_started, now));
    }

    #[test]
    fn a_camera_that_has_never_delivered_gets_the_warmup_then_gives_up() {
        let now = Instant::now();
        assert!(!is_stalled(None, now - Duration::from_secs(19), now));
        assert!(is_stalled(None, now - Duration::from_secs(21), now));
    }
}
