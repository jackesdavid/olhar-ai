//! Persistent per-camera frame decoder (#120). One **supervised**, long-lived decoder process reads
//! go2rtc's WARM RTSP re-serve (`rtsp://127.0.0.1:8554/cam_<ip>[_hd]`) → a continuous MJPEG stream, so
//! the frame hub reads the freshest decoded frame with **no per-request `frame.jpeg` cold-start**
//! (which measured ~490 ms/grab). The consumer keeps go2rtc's producer warm on its own.
//!
//! **Two interchangeable [`Backend`]s** (card #138, `docs/plans/edge-gstreamer-backend.md`): `ffmpeg`
//! (default) or `gstreamer`, picked per box via `RecordingConfig.media_backend`. Both emit the SAME
//! wire format — concatenated JPEG images on stdout — so [`read_mjpeg`] doesn't care which produced
//! them. This is a coexistence/measurement phase, not a permanent fork; see the plan doc.
//!
//! **Resilient by design:** the decoder exits (stream drop) OR wedges (alive but no frames) are both
//! detected — a watchdog kills a stalled process so the read unblocks, and the supervisor respawns
//! with exponential backoff. The hub always falls back to go2rtc `frame.jpeg` when there's no FRESH
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
use std::process::{Child, Stdio};
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

/// Which decoder slot a request should join. Unified never opens a second main-stream session.
pub fn stream_slot(state: &crate::state::AppState, ip: &str, hd: bool) -> bool {
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

use crate::media::Backend;

/// Ensure a supervised decoder is running for `ip` on the requested stream. Idempotent **per
/// slot** — Split mode can run preview (sub) and HD (main) at once; Unified keeps a single
/// main-only slot. `media_backend` is the box's `RecordingConfig.media_backend` ("ffmpeg" or
/// "gstreamer", card #138).
pub fn ensure(state: &crate::state::AppState, ip: &str, hd: bool) {
    let slot = stream_slot(state, ip, hd);
    let cam = cam_for(ip, slot);
    if cam.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let backend = Backend::resolve(&state.recording.config.media_backend);
    // The linked path dials the camera itself; the ffmpeg path still goes through go2rtc's re-serve,
    // which is the whole difference between owning the ingest and renting it.
    // `slot` (not the caller's raw `hd`) picks the URL — unified always resolves to main inside
    // `camera_rtsp` via `Device::is_unified`, even when the slot is the preview key.
    let url = match backend {
        Backend::GstreamerLinked => state.camera_rtsp(ip, slot),
        _ => None,
    };
    ensure_started(ip, slot, cam, backend, url);
}

/// Start the supervisor for a camera whose RTSP URL the caller already has — the same path as
/// [`ensure`], minus the `AppState` lookup, so it can be driven from a test.
fn ensure_started(ip: &str, hd: bool, cam: Arc<Cam>, backend: Backend, url: Option<String>) {
    let ip = ip.to_string();
    std::thread::spawn(move || supervise(&ip, hd, cam, backend, url));
}

/// Test-only entry point: dial `url` directly for `ip`, bypassing device state.
#[cfg(test)]
fn ensure_with_url(ip: &str, hd: bool, backend: &str, url: &str) {
    let cam = cam_for(ip, hd);
    if cam.started.swap(true, Ordering::SeqCst) {
        return;
    }
    ensure_started(
        ip,
        hd,
        cam,
        Backend::resolve(backend),
        Some(url.to_string()),
    );
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
/// else `None` so the hub falls back to go2rtc `frame.jpeg`.
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
        app_core::go2rtc::stream_name_hd(ip)
    } else {
        app_core::go2rtc::stream_name(ip)
    };
    // go2rtc's LOCAL RTSP re-serve (creds already injected) — localhost, warm passthrough.
    format!("rtsp://127.0.0.1:8554/{name}")
}

/// Build the full arg list for the decode ffmpeg, optionally with GPU decode (`use_hwaccel`).
/// `-hwaccel …` must precede `-i`; the download filter (if any) goes on the encode side, right
/// before the output format.
fn ffmpeg_args(ip: &str, hd: bool, use_hwaccel: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    if use_hwaccel {
        args.extend(crate::hwaccel::decode_args().iter().map(|s| s.to_string()));
    }
    args.extend([
        "-rtsp_transport".into(),
        "tcp".into(),
        "-i".into(),
        stream_url(ip, hd),
    ]);
    args.extend([
        "-an".into(),
        "-r".into(),
        "6".into(),
        "-q:v".into(),
        "2".into(),
    ]);
    if use_hwaccel {
        if let Some(filter) = crate::hwaccel::decode_filter() {
            args.extend(["-vf".into(), filter.to_string()]);
        }
    }
    args.extend(["-f".into(), "mjpeg".into(), "pipe:1".into()]);
    args
}

fn spawn_ffmpeg(ip: &str, hd: bool, use_hwaccel: bool) -> std::io::Result<Child> {
    // Decode the warm RTSP → MJPEG at a modest fps (the hub throttles to ~4fps; 6 keeps frames fresh
    // without over-encoding). High JPEG quality (-q:v 2) so the derived face crops stay sharp.
    // `use_hwaccel` asks for GPU decode via `crate::hwaccel` when the box detected one (#120 probe);
    // `supervise()` turns it off for a camera whose GPU decode fails in practice even though the
    // synthetic probe passed (e.g. a driver that only breaks under real load).
    crate::proc::command("ffmpeg")
        .args(ffmpeg_args(ip, hd, use_hwaccel))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
}

/// Same job as [`spawn_ffmpeg`] (warm RTSP → concatenated-JPEG on stdout at ~6fps), via GStreamer
/// (card #138). `decodebin` auto-negotiates depay/parse/decoder for whatever codec the camera's
/// stream actually carries (H264 or H265) — codec-agnostic, mirroring ffmpeg's own auto-detection —
/// so this one pipeline covers every camera without a per-codec branch. `videoconvert` normalizes
/// whatever raw format `decodebin` lands on (I420/NV12/…) to what `jpegenc` expects.
///
/// **Software decode only for now** (Phase 2a of the plan) — no HW-decode element
/// (`nvv4l2decoder`/`vaapih264dec`/`vtdec`) is selected yet, so the first ffmpeg-vs-gstreamer
/// measurement is apples-to-apples (both CPU-only). Mapping the box's detected [`crate::hwaccel`]
/// method to a GStreamer HW element is Phase 3, once SW parity is measured.
///
/// `GST_PLUGIN_FEATURE_RANK` zero-ranks the known HW-decoder elements so `decodebin` can't
/// autoplug one even if it's installed and otherwise the highest-ranked H264/H265 decoder on the
/// box — found live on the Jetson test box: `videoconvert` only accepts system-memory
/// `video/x-raw`, but once `gstreamer1.0-plugins-bad` is present too (installed by `install.sh`
/// for a future Phase 3), `decodebin` prefers `nvv4l2decoder`, whose output is Tegra `NVMM`
/// memory — `videoconvert` can't take that, so the pipeline fails outright with
/// `not-negotiated`, for EVERY camera. Without this pin, capture silently breaks the moment the
/// box happens to have any GPU-decoder GStreamer plugin installed.
fn spawn_gst(ip: &str, hd: bool) -> std::io::Result<Child> {
    let pipeline = format!(
        "rtspsrc location={} protocols=tcp latency=200 ! decodebin ! videoconvert ! videorate ! \
         video/x-raw,framerate=6/1 ! jpegenc quality=85 ! fdsink fd=1 sync=false",
        stream_url(ip, hd)
    );
    crate::proc::command("gst-launch-1.0")
        .arg("-q") // quiet: no progress/property spam on stdout — only the fdsink's JPEG bytes
        .args(pipeline.split_whitespace())
        .env(
            "GST_PLUGIN_FEATURE_RANK",
            "nvv4l2decoder:0,nvv4l2h264dec:0,nvv4l2h265dec:0,vaapih264dec:0,vaapih265dec:0,\
             v4l2h264dec:0,v4l2h265dec:0,msdkh264dec:0,msdkh265dec:0,d3d11h264dec:0,d3d11h265dec:0,\
             vtdec_hw:0",
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
}

fn spawn(ip: &str, hd: bool, backend: Backend, use_hwaccel: bool) -> std::io::Result<Child> {
    match backend {
        Backend::Ffmpeg => spawn_ffmpeg(ip, hd, use_hwaccel),
        Backend::Gstreamer => spawn_gst(ip, hd),
        // GstreamerLinked never reaches spawn() — supervise() runs it in-process, not as a Child.
        // Kept for match exhaustiveness; ffmpeg is the safe fallback if it somehow does.
        Backend::GstreamerLinked => spawn_ffmpeg(ip, hd, use_hwaccel),
    }
}

/// The linked `gstreamer-rs` A1 path (card #138) — the ffmpeg replacement. Runs the decode pipeline
/// **in-process** (no subprocess), publishing each JPEG to `cam.latest` via `appsink`, keeping the
/// exact MJPEG output contract so every downstream consumer (snapshot, detector, hub) is unchanged.
/// `decodebin` autoplugs the platform HW decoder for a real camera stream (`vtdec` on Mac); on the
/// Jetson the `nvv4l2decoder → NVMM` memory needs `nvvidconv` in place of `videoconvert` — that
/// swap is the Jetson gate's job. Blocks until the pipeline errors, then returns so `supervise()`
/// restarts it under the same backoff policy as the ffmpeg path.
fn run_gst_linked(ip: &str, hd: bool, cam: &Arc<Cam>, url: Option<&str>) -> Result<(), String> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;

    crate::media::init_gstreamer()?; // init + iOS static plugin registration
                                     // The camera's own URL when we have it; go2rtc's re-serve only as a fallback for a device whose
                                     // RTSP never resolved. Owning the dial is what lets the sidecar go away.
    let url = url
        .map(str::to_string)
        .unwrap_or_else(|| stream_url(ip, hd));

    // The RTSP INGEST is platform-specific; everything from `h264parse` on is identical.
    //
    // On **Jetson/Tegra**, GStreamer 1.20's `rtspsrc` cannot complete some cameras' RTSP Digest-auth
    // handshake — it finishes OPTIONS + auth then stalls at SETUP and loops forever, holding a dead
    // session. Verified against a live Dahua: EVERY `rtspsrc` transport/URL/auth combination = 0
    // bytes, while `retina` (and VLC/ffmpeg) streamed it fine. The upstream fix, `rtspsrc2`, needs
    // GStreamer ≥1.22, which L4T can't provide without risking the NVIDIA NVDEC plugins. So on Tegra
    // we pull with **retina** — the pure-Rust RTSP client the recording ring already uses — and feed
    // the H264 into the SAME graph via `appsrc` (proven end-to-end on the Orin Nano: NVDEC decode +
    // mp4mux both work). Every OTHER platform keeps `rtspsrc` untouched.
    let use_retina = crate::media::is_tegra();

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
    let source = if use_retina {
        // Fed by the retina thread below. `do-timestamp` stamps each pushed access unit so `mp4mux`
        // gets the PTS it needs (retina gives us bare access units, no container timing).
        "appsrc name=src is-live=true do-timestamp=true format=time".to_string()
    } else {
        format!(
            "rtspsrc location=\"{url}\" protocols=tcp latency=200 ! {}",
            codec.depay_element()
        )
    };
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

    // Tegra: spawn the retina feeder that dials the camera and pushes H264 access units into
    // `appsrc`. It runs a small current-thread tokio runtime (like the recording ring). On any retina
    // error / stop it ends the appsrc stream so the bus loop below unblocks and `supervise` restarts.
    let feeder_stop = Arc::new(AtomicBool::new(false));
    let feeder = if use_retina {
        let appsrc = pipeline
            .by_name("src")
            .ok_or("appsrc missing")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "src is not an appsrc".to_string())?;
        let url_c = url.clone();
        let ip_c = ip.to_string();
        let stop_c = Arc::clone(&feeder_stop);
        Some(std::thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => {
                    if let Err(e) = rt.block_on(retina_feed(&url_c, &appsrc, &stop_c, codec)) {
                        log::warn!("capture-decoder[{ip_c}]: retina ingest: {e}");
                    }
                }
                Err(e) => log::error!("capture-decoder[{ip_c}]: retina runtime: {e}"),
            }
            let _ = appsrc.end_of_stream(); // unblock the bus loop → supervise restarts
        }))
    } else {
        None
    };

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
        let stalled = match &*cam.latest.lock().unwrap() {
            Some((_, at)) => at.elapsed() > Duration::from_secs(12),
            None => started.elapsed() > Duration::from_secs(20),
        };
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
    feeder_stop.store(true, Ordering::Relaxed);
    let _ = pipeline.set_state(gst::State::Null);
    if let Some(h) = feeder {
        let _ = h.join();
    }
    result
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

/// The Tegra RTSP ingest: pull codec-framed access units with `retina` and push them into `appsrc`.
/// Mirrors the PoC verified on the Orin Nano. `retina` handles the Digest handshake GStreamer's
/// `rtspsrc` can't; its `demuxed()` gives us access units, and the SDP's `codec_data` goes into the
/// appsrc caps so the parser negotiates avc/hvc1 without waiting for inline parameter sets. `codec`
/// (from [`probe_codec`]) selects H264 vs H265.
async fn retina_feed(
    url: &str,
    appsrc: &gstreamer_app::AppSrc,
    stop: &AtomicBool,
    codec: VideoCodec,
) -> Result<(), String> {
    use futures::StreamExt;
    use gstreamer as gst;

    let (bare, creds) = crate::recording::split_rtsp_credentials(url);
    let parsed = bare
        .parse::<url::Url>()
        .map_err(|e| format!("bad url: {e}"))?;
    let opts = match creds {
        Some((username, password)) => retina::client::SessionOptions::default()
            .creds(Some(retina::client::Credentials { username, password })),
        None => retina::client::SessionOptions::default(),
    };
    let mut session = retina::client::Session::describe(parsed, opts)
        .await
        .map_err(|e| format!("describe: {e}"))?;
    let vid = session
        .streams()
        .iter()
        .position(|s| {
            s.media() == "video" && VideoCodec::from_encoding(s.encoding_name()) == Some(codec)
        })
        .ok_or_else(|| format!("no {} video stream", codec.label()))?;
    // codec_data (AVCDecoderConfigurationRecord / HEVCDecoderConfigurationRecord) → caps for the appsrc.
    let extra: Vec<u8> = match session.streams()[vid].parameters() {
        Some(retina::codec::ParametersRef::Video(v)) => v.extra_data().to_vec(),
        _ => Vec::new(),
    };
    let mut caps = gst::Caps::builder(codec.caps_media())
        .field("stream-format", codec.stream_format())
        .field("alignment", "au");
    if !extra.is_empty() {
        caps = caps.field("codec_data", gst::Buffer::from_slice(extra));
    }
    appsrc.set_caps(Some(&caps.build()));

    session
        .setup(vid, retina::client::SetupOptions::default())
        .await
        .map_err(|e| format!("setup: {e}"))?;
    let playing = session
        .play(retina::client::PlayOptions::default())
        .await
        .map_err(|e| format!("play: {e}"))?;
    let mut demuxed = playing.demuxed().map_err(|e| format!("demuxed: {e}"))?;

    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // A 5s read timeout doubles as a liveness check: a camera that stops sending unblocks us so
        // the feeder ends and the pipeline restarts (the watchdog would also catch it downstream).
        match tokio::time::timeout(Duration::from_secs(5), demuxed.next()).await {
            Ok(Some(Ok(retina::codec::CodecItem::VideoFrame(f)))) => {
                let data = f.into_data();
                let key = is_keyframe(&data, codec);
                let mut buf = gst::Buffer::from_mut_slice(data);
                // The appsrc's declared caps already match the mux branch's desired output
                // (avc/hvc1 + alignment=au), so both `{parse}` instances downstream run in
                // PASSTHROUGH — neither does bitstream analysis, so whatever sync-point flag
                // we set here is exactly what `mp4mux` sees. Without this, every buffer defaults
                // to "keyframe" (`DELTA_UNIT` unset on a fresh `Buffer`), which happens to work
                // by accident for H264, but see the `is_keyframe` doc for why H265 needs it set
                // explicitly and correctly.
                if !key {
                    buf.get_mut()
                        .expect("buffer uniquely owned right after creation")
                        .set_flags(gst::BufferFlags::DELTA_UNIT);
                }
                if appsrc.push_buffer(buf).is_err() {
                    return Ok(()); // pipeline gone; not an error worth logging
                }
            }
            Ok(Some(Ok(_))) => {} // RTCP / parameter updates
            Ok(Some(Err(e))) => return Err(format!("packet: {e}")),
            Ok(None) => return Err("stream ended".to_string()),
            Err(_) => return Err("frame read timeout".to_string()),
        }
    }
}

/// Whether `au` (one retina access unit, four-byte-length-prefixed NALs per
/// `retina::codec::VideoFrame::into_data`) contains an intra random-access-point NAL — i.e.
/// whether `mp4mux` can start a new fragment here.
///
/// Determined by direct NAL-type inspection rather than trusting `retina`'s own
/// `VideoFrame::is_random_access_point()`: that call only recognizes H.265 IDR (NAL types 19/20)
/// and misses the BLA/CRA types (16-18, 21-23) many camera encoders use for periodic refresh
/// instead of IDR (confirmed live on a Dahua substream, and a known upstream gap —
/// https://github.com/scottlamb/retina/issues/132). Miss every keyframe and `mp4mux`'s
/// `streamable=true` fragmenting never sees a sync sample to start a fragment on — the pipeline
/// doesn't error, it just never produces the first fragment, forever (`live-push`/`mse` then log
/// "produced no fragment within 10s"). H264 IDR (type 5) isn't affected by that particular bug,
/// but is checked here too — same cost, one fewer place a future codec-detection change could
/// silently reintroduce this.
fn is_keyframe(au: &[u8], codec: VideoCodec) -> bool {
    let mut i = 0usize;
    while i + 4 <= au.len() {
        let len = u32::from_be_bytes([au[i], au[i + 1], au[i + 2], au[i + 3]]) as usize;
        let nal = i + 4;
        if len == 0 || nal >= au.len() {
            break;
        }
        let header = au[nal];
        let is_irap = match codec {
            VideoCodec::H264 => (header & 0x1F) == 5, // IDR
            // H.265 NAL type is bits 1-6 of the (2-byte) header's first byte. 16-23 is the full
            // IRAP range (BLA_W_LP..RSV_IRAP_VCL23) — the set `retina` under-recognizes.
            VideoCodec::H265 => {
                let nal_type = (header >> 1) & 0x3F;
                (16..=23).contains(&nal_type)
            }
        };
        if is_irap {
            return true;
        }
        i = (nal + len).min(au.len());
    }
    false
}

/// Consecutive fast (<30s) failures while GPU decode is on before we give up on it for this
/// camera and fall back to CPU. The synthetic probe in `hwaccel::detect()` (open the hw device
/// once, decode one null frame) can pass while the real thing still fails under load — a driver
/// that only breaks on a genuine RTSP stream, or a codec profile it doesn't actually support.
const HWACCEL_FAIL_LIMIT: u32 = 2;

fn supervise(ip: &str, hd: bool, cam: Arc<Cam>, backend: Backend, url: Option<String>) {
    log::info!(
        "capture-decoder[{ip}]: starting (stream={}, backend={})",
        if hd { "HD (main)" } else { "preview (sub)" },
        backend.label()
    );
    let mut backoff = Duration::from_millis(500);
    let mut fails: u32 = 0;
    // GPU-decode fallback bookkeeping only applies to the ffmpeg branch — the gstreamer branch
    // (Phase 2a) is software-only by design, so there's nothing to fall back from there yet.
    let mut use_hwaccel = backend == Backend::Ffmpeg && crate::hwaccel::detect() != "none";
    let mut hw_fails: u32 = 0;
    loop {
        let ran_from = Instant::now();

        // Linked gstreamer-rs path (card #138) runs in-process, not as a Child — handle it before
        // the subprocess spawn below. It shares this loop's backoff/restart policy verbatim.
        if backend == Backend::GstreamerLinked {
            if let Err(e) = run_gst_linked(ip, hd, &cam, url.as_deref()) {
                log::warn!("capture-decoder[{ip}]: gstreamer-linked: {e}");
            }
        } else {
            match spawn(ip, hd, backend, use_hwaccel) {
                Ok(mut child) => {
                    let stdout = match child.stdout.take() {
                        Some(s) => s,
                        None => {
                            let _ = child.kill();
                            let _ = child.wait();
                            std::thread::sleep(backoff);
                            continue;
                        }
                    };
                    let spawn_at = Instant::now();
                    let child = Arc::new(Mutex::new(child));
                    // Watchdog: kill a wedged ffmpeg (alive but not producing) so the read below unblocks.
                    // Generous warmup (HD streams take a moment); silent (the supervisor logs the policy).
                    let wd = {
                        let (cam_w, child_w) = (Arc::clone(&cam), Arc::clone(&child));
                        std::thread::spawn(move || loop {
                            std::thread::sleep(Duration::from_secs(2));
                            let mut c = child_w.lock().unwrap();
                            match c.try_wait() {
                                Ok(Some(_)) => break, // exited on its own → reader will EOF
                                Ok(None) => {
                                    let stale = match &*cam_w.latest.lock().unwrap() {
                                        Some((_, at)) => at.elapsed() > Duration::from_secs(8),
                                        None => spawn_at.elapsed() > Duration::from_secs(20),
                                    };
                                    if stale {
                                        let _ = c.kill();
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        })
                    };
                    read_mjpeg(stdout, &cam); // blocks until EOF (natural exit or watchdog kill)
                    {
                        let mut c = child.lock().unwrap();
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    let _ = wd.join();
                }
                Err(e) => log::warn!(
                    "capture-decoder[{ip}]: {} spawn failed: {e}",
                    backend.label()
                ),
            }
        }
        // Backoff policy (the fix): reset ONLY after a healthy run — a fast-dying ffmpeg must NOT reset
        // the backoff (that was the tight-loop storm). Repeated fast failures cool down HARD so we
        // never hammer go2rtc; the hub keeps working on the frame.jpeg fallback the whole time.
        if ran_from.elapsed() >= Duration::from_secs(30) {
            if fails > 0 {
                log::info!("capture-decoder[{ip}]: recovered — streaming again");
            }
            backoff = Duration::from_millis(500);
            fails = 0;
            hw_fails = 0; // a healthy run vindicates GPU decode too — don't carry stale fails
        } else {
            fails += 1;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            if fails == 5 {
                log::warn!(
                    "capture-decoder[{ip}]: repeated {} failures — backing off to 30s (frame.jpeg fallback active)",
                    backend.label()
                );
            }
            // GPU decode fast-failing in practice despite passing the boot-time probe (#120) —
            // fall back to CPU for this camera rather than retrying the same broken command forever.
            if use_hwaccel {
                hw_fails += 1;
                if hw_fails >= HWACCEL_FAIL_LIMIT {
                    use_hwaccel = false;
                    log::warn!(
                        "capture-decoder[{ip}]: GPU decode ({}) failing in practice — falling back to CPU decode for this camera",
                        crate::hwaccel::detect()
                    );
                }
            }
        }
        std::thread::sleep(backoff);
    }
}

/// Read concatenated MJPEG (SOI `FF D8` … EOI `FF D9`) from ffmpeg's stdout, publishing each complete
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

#[cfg(all(test, feature = "gst-linked"))]
mod is_keyframe_tests {
    use super::{is_keyframe, VideoCodec};

    /// Builds one retina-style access unit: 4-byte big-endian length + NAL bytes, whose first
    /// byte carries `nal_header` (H264: `nal_unit_type` in bits 0-4; H265: `nal_unit_type` in
    /// bits 1-6 of the FIRST of its two header bytes).
    fn au(nal_header: u8, rest: &[u8]) -> Vec<u8> {
        let mut nal = vec![nal_header];
        nal.extend_from_slice(rest);
        if matches!(rest.first(), None) {
            nal.push(0); // H265's 2-byte header — pad so real slice data isn't required
        }
        let mut out = (nal.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&nal);
        out
    }

    #[test]
    fn h264_idr_is_a_keyframe() {
        let data = au(0x65, &[0x88]); // nal_unit_type = 5 (IDR), nal_ref_idc = 3
        assert!(is_keyframe(&data, VideoCodec::H264));
    }

    #[test]
    fn h264_non_idr_is_not_a_keyframe() {
        let data = au(0x41, &[0x9a]); // nal_unit_type = 1 (non-IDR slice)
        assert!(!is_keyframe(&data, VideoCodec::H264));
    }

    #[test]
    fn h265_idr_is_a_keyframe() {
        // nal_unit_type = 19 (IDR_W_RADL) in bits 1-6: header byte = 19 << 1 = 0x26.
        let data = au(0x26, &[]);
        assert!(is_keyframe(&data, VideoCodec::H265));
    }

    #[test]
    fn h265_cra_is_a_keyframe() {
        // The exact case `retina::VideoFrame::is_random_access_point()` misses (#132): CRA_NUT
        // is NAL type 21 → header byte = 21 << 1 = 0x2a. Cameras that refresh via CRA instead of
        // IDR (verified live on a Dahua) would otherwise never produce a fragment, ever.
        let data = au(0x2a, &[]);
        assert!(is_keyframe(&data, VideoCodec::H265));
    }

    #[test]
    fn h265_trail_r_is_not_a_keyframe() {
        // TRAIL_R = NAL type 1 → header byte = 1 << 1 = 0x02.
        let data = au(0x02, &[]);
        assert!(!is_keyframe(&data, VideoCodec::H265));
    }

    #[test]
    fn multiple_nals_in_one_au_are_all_scanned() {
        // A real AU can carry VPS+SPS+PPS+slice concatenated; the slice's IRAP-ness is what
        // matters, wherever in the AU it lands.
        let vps = au(0x40, &[]); // VPS_NUT = 32 → 0x40
        let mut data = vps;
        data.extend(au(0x2a, &[])); // CRA slice appended after it
        assert!(is_keyframe(&data, VideoCodec::H265));
    }
}

#[cfg(all(test, feature = "gst-linked"))]
mod gst_linked_it {
    //! Integration test for the shared ingest (card #138, G1/A1/G2): ONE camera connection, `tee`'d
    //! to the decode branch (JPEG for motion + detectors) and the mux branch (fMP4 for live
    //! viewers). Both must come out of the same pipeline — that is the whole point, so both are
    //! asserted here. Environment-specific, so `#[ignore]`d:
    //!   GST_IT_CAM_URL='rtsp://user:pass@host/streamN' cargo test --features gst-linked -- --ignored gst_linked
    use std::time::Duration;

    #[test]
    #[ignore = "needs a live camera"]
    fn one_connection_feeds_both_the_decoder_and_the_live_muxer() {
        let url =
            std::env::var("GST_IT_CAM_URL").expect("set GST_IT_CAM_URL to a live camera RTSP URL");
        let ip = "10.255.255.1"; // a key, not a dial — the URL above is what gets connected
        super::ensure_with_url(ip, false, "gstreamer-linked", &url);

        // Decode branch: a fresh JPEG lands in the slot every consumer reads.
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

        // Mux branch: an init segment a browser can start on, then live fragments.
        let feed = super::mse_feed(ip, false).expect("mux branch produced no feed");
        let (init, mime, mut rx) = feed;
        assert_eq!(&init[4..8], b"ftyp", "init segment must open with ftyp");
        assert!(
            init.windows(4).any(|w| w == b"moov"),
            "init segment must carry moov"
        );
        assert!(
            mime.starts_with("video/mp4; codecs=\"avc1."),
            "unexpected mime: {mime}"
        );

        let mut got = 0;
        while got < 3 {
            rx.blocking_recv().expect("fragment channel closed");
            got += 1;
        }
    }
}
