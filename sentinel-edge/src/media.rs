//! Shared media-backend selection (card #138, `docs/plans/edge-gstreamer-backend.md`): which
//! external media tool a box's decode/mux/snapshot operations shell out to. One flag
//! (`RecordingConfig.media_backend`), consulted independently at each touchpoint (`capture.rs`
//! A1, `recording.rs` A2/A3, `app_core::onvif` A5) so a partial rollout (e.g. gstreamer only
//! proven for live decode so far) never has to be all-or-nothing.

/// Which tool to shell out to. Both branches, at every touchpoint, must produce
/// byte-for-byte-compatible OUTPUT (same wire format / same file format) — callers must not
/// care which one ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Ffmpeg,
    Gstreamer,
    /// **Linked `gstreamer-rs`** (card #138 pivot) — the successor to the ffmpeg A1 path, not a
    /// permanent parallel: once validated on every target it becomes the default and the ffmpeg A1
    /// path is removed. Unlike `Gstreamer` (a spawned `gst-launch` subprocess), this links
    /// libgstreamer and runs the pipeline in-process (the only model that ports to Android). The
    /// implementation lives behind the `gst-linked` build feature; without that feature `resolve`
    /// never returns this variant.
    GstreamerLinked,
}

impl Backend {
    /// Resolve the config string, falling back to `Ffmpeg` for anything unrecognized AND for
    /// `"gstreamer"` on a box that doesn't actually have `gst-launch-1.0` on PATH — a stale or
    /// mistaken flag must never leave an operation with no working tool
    /// (`state::gstreamer_ok()` mirrors `ffmpeg_ok()`).
    pub fn resolve(config_value: &str) -> Self {
        // Linked path (card #138). Only available when this build has the `gst-linked` feature;
        // otherwise fall through to ffmpeg so a stale flag never strands a box.
        if config_value.eq_ignore_ascii_case("gstreamer-linked") {
            #[cfg(feature = "gst-linked")]
            return Backend::GstreamerLinked;
            #[cfg(not(feature = "gst-linked"))]
            log::warn!(
                "media_backend=gstreamer-linked but this build lacks the `gst-linked` feature — using ffmpeg"
            );
        }
        if config_value.eq_ignore_ascii_case("gstreamer") {
            if crate::state::gstreamer_ok() {
                return Backend::Gstreamer;
            }
            log::warn!(
                "media_backend=gstreamer requested but gst-launch-1.0 isn't on PATH — using ffmpeg"
            );
        }
        Backend::Ffmpeg
    }

    pub fn label(self) -> &'static str {
        match self {
            Backend::Ffmpeg => "ffmpeg",
            Backend::Gstreamer => "gstreamer",
            Backend::GstreamerLinked => "gstreamer-linked",
        }
    }
}

/// The decode half of the ingest pipeline, chosen from what this box's GStreamer registry actually
/// has (card #138, A1/HW + A4).
///
/// **Why probe the registry instead of naming a platform.** The same binary ships to a Mac, a
/// Jetson, a Linux server and eventually Android and Windows, and each has a different hardware
/// decoder under a different element name. Asking the registry is both the capability probe (A4,
/// replacing `ffmpeg -hwaccels`) and the selection — one source of truth, and a box missing its
/// accelerator degrades to software instead of failing to start.
///
/// The Jetson's `nvv4l2decoder` outputs into **NVMM** (GPU) memory, which `videoconvert` cannot
/// read — it needs `nvvidconv` to reach system memory. Pairing the wrong converter is not a
/// negotiation error you can miss: the pipeline simply never produces a frame.
#[cfg(feature = "gst-linked")]
pub fn decode_branch() -> &'static str {
    use gstreamer as gst;
    let has = |name: &str| gst::ElementFactory::find(name).is_some();
    if has("nvv4l2decoder") && has("nvvidconv") {
        // Jetson / Tegra: NVDEC into NVMM, then the NVIDIA converter back to system memory.
        "nvv4l2decoder ! nvvidconv"
    } else if has("amcviddec-omxh264dec") || has("amcviddec-c2avcdecoder") {
        // Android MediaCodec — `decodebin` picks it, but say so explicitly for the log.
        "decodebin ! videoconvert"
    } else {
        // Mac (VideoToolbox `vtdec`), Linux (VAAPI / NVDEC), Windows (D3D11): `decodebin` already
        // picks the best registered decoder, verified on this Mac.
        "decodebin ! videoconvert"
    }
}

/// Which hardware decoder this box actually has, for the health report (A4 — the `ffmpeg -hwaccels`
/// replacement). `"none"` means software decode, which still works, just warmer.
#[cfg(feature = "gst-linked")]
pub fn hw_decoder() -> &'static str {
    use gstreamer as gst;
    let has = |name: &str| gst::ElementFactory::find(name).is_some();
    // Ordered most- to least-specific: a box with a dedicated decoder should report it, not the
    // generic one it also happens to carry.
    for (element, label) in [
        ("nvv4l2decoder", "nvdec-tegra"),
        ("nvh264dec", "nvdec"),
        ("vtdec_hw", "videotoolbox"),
        ("vtdec", "videotoolbox"),
        ("vaapih264dec", "vaapi"),
        ("d3d11h264dec", "d3d11"),
    ] {
        if has(element) {
            return label;
        }
    }
    "none"
}

/// Prove this box's GStreamer runtime can build the ingest (card #138) by instantiating every
/// element the REAL pipeline uses — constructing an element `dlopen`s its plugin, so a missing or
/// broken plugin fails right here, named. Deliberately uses only production elements: no
/// `videotestsrc`/`x264enc`, which would drag `libx264` (GPL) into a bundle we redistribute. The
/// macOS installer runs this in its postinstall to reject an incomplete bundle before trusting a box
/// to it, instead of discovering the gap the first time a camera connects.
#[cfg(feature = "gst-linked")]
pub fn gst_selftest() -> Result<(), String> {
    use gstreamer as gst;
    init_gstreamer()?;

    // Exactly what `capture.rs`'s pipeline instantiates. Keep in step with it.
    let required = [
        "rtspsrc", "rtph264depay", "h264parse", "tee", "queue", "capsfilter", "decodebin",
        "videoconvert", "videorate", "jpegenc", "mp4mux", "appsink", "fakesink",
    ];
    for name in required {
        gst::ElementFactory::make(name)
            .build()
            .map_err(|_| format!("cannot create `{name}` — its plugin is missing or failed to load"))?;
    }

    // The hardware decoder is not fatal — the pipeline falls back to `decodebin` software decode —
    // but a box that reports one it cannot build would lie on its health badge, so verify it too.
    let hw = hw_decoder();
    if hw != "none" {
        let element = match hw {
            "nvdec-tegra" => "nvv4l2decoder",
            "nvdec" => "nvh264dec",
            "videotoolbox" => "vtdec",
            "vaapi" => "vaapih264dec",
            "d3d11" => "d3d11h264dec",
            _ => "",
        };
        if !element.is_empty() && gst::ElementFactory::make(element).build().is_err() {
            return Err(format!("hardware decoder `{element}` is advertised but will not build"));
        }
    }
    Ok(())
}

/// Initialise GStreamer, registering the statically-linked plugins on platforms where `gst_init`
/// cannot discover them itself (card #144). On the Mac/Linux/Jetson the plugins are separate dylibs
/// that `gst_init` scans and loads; **on iOS they are statically linked into one framework**, so
/// each plugin our pipelines use must be registered by hand — otherwise `gst_init` succeeds but
/// `rtspsrc`, `mp4mux`, … simply do not exist. Idempotent; call this instead of `gst::init()`.
#[cfg(feature = "gst-linked")]
pub fn init_gstreamer() -> Result<(), String> {
    gstreamer::init().map_err(|e| format!("gst init: {e}"))?;
    #[cfg(target_os = "ios")]
    ios_static_plugins::register_once();
    Ok(())
}

/// The plugins our pipelines need — the visible elements plus rtspsrc's hidden deps (rtpmanager, udp), registered from their static
/// `gst_plugin_<name>_register` symbols in the iOS GStreamer framework. Keep this list in step with
/// the pipeline (and with the Mac `.pkg`'s bundled-plugin list).
#[cfg(all(feature = "gst-linked", target_os = "ios"))]
mod ios_static_plugins {
    use std::sync::Once;

    // gboolean gst_plugin_<name>_register(void) — present in GStreamer.framework (verified with nm).
    extern "C" {
        fn gst_plugin_coreelements_register() -> i32;
        fn gst_plugin_rtsp_register() -> i32;
        fn gst_plugin_rtp_register() -> i32;
        fn gst_plugin_isomp4_register() -> i32;
        fn gst_plugin_jpeg_register() -> i32;
        fn gst_plugin_playback_register() -> i32;
        fn gst_plugin_videoconvertscale_register() -> i32;
        fn gst_plugin_videoparsersbad_register() -> i32;
        fn gst_plugin_videorate_register() -> i32;
        fn gst_plugin_typefindfunctions_register() -> i32;
        fn gst_plugin_autodetect_register() -> i32;
        fn gst_plugin_app_register() -> i32;
        fn gst_plugin_applemedia_register() -> i32;
        fn gst_plugin_rtpmanager_register() -> i32;
        fn gst_plugin_udp_register() -> i32;
    }

    pub fn register_once() {
        static REGISTERED: Once = Once::new();
        REGISTERED.call_once(|| unsafe {
            gst_plugin_coreelements_register();
            gst_plugin_rtsp_register();
            gst_plugin_rtp_register();
            gst_plugin_isomp4_register();
            gst_plugin_jpeg_register();
            gst_plugin_playback_register();
            gst_plugin_videoconvertscale_register();
            gst_plugin_videoparsersbad_register();
            gst_plugin_videorate_register();
            gst_plugin_typefindfunctions_register();
            gst_plugin_autodetect_register();
            gst_plugin_app_register();
            gst_plugin_applemedia_register();
            gst_plugin_rtpmanager_register(); // rtspsrc's RTP session (rtpbin/jitterbuffer)
            gst_plugin_udp_register();          // udpsrc/udpsink for RTP-over-UDP
        });
    }
}

/// Dev-only: dial an RTSP camera directly and record `secs` seconds to `out` as MP4 (card #144).
/// Proves the embedded phone edge can ingest + decode + mux a REAL camera on-device, without any
/// enrollment/box plumbing. `rtspsrc ! rtph264depay ! h264parse ! mp4mux ! filesink`, then a clean
/// EOS so mp4mux writes its moov. Returns (bytes, elapsed_secs).
#[cfg(feature = "gst-linked")]
pub fn record_camera(url: &str, secs: u64, out: &std::path::Path) -> Result<(u64, u64), String> {
    use gstreamer::prelude::*;
    init_gstreamer()?;

    let make = |factory: &str| {
        gstreamer::ElementFactory::make(factory)
            .build()
            .map_err(|_| format!("cannot create `{factory}`"))
    };
    let pipeline = gstreamer::Pipeline::new();
    let src = gstreamer::ElementFactory::make("rtspsrc")
        .property("location", url)
        .property("latency", 200u32)
        .property_from_str("protocols", "tcp")
        .build()
        .map_err(|_| "cannot create `rtspsrc`".to_string())?;
    let depay = make("rtph264depay")?;
    let parse = make("h264parse")?;
    let mux = gstreamer::ElementFactory::make("mp4mux")
        .property("faststart", true)
        .build()
        .map_err(|_| "cannot create `mp4mux`".to_string())?;
    let sink = gstreamer::ElementFactory::make("filesink")
        .property("location", out.to_string_lossy().as_ref())
        .build()
        .map_err(|_| "cannot create `filesink`".to_string())?;

    pipeline
        .add_many([&src, &depay, &parse, &mux, &sink])
        .map_err(|e| format!("add: {e}"))?;
    gstreamer::Element::link_many([&depay, &parse, &mux, &sink])
        .map_err(|e| format!("link: {e}"))?;

    // rtspsrc exposes a pad per stream only after it reads the SDP (video + maybe audio), so link
    // the depay dynamically — the canonical, camera-agnostic way (a static parse link mis-negotiates
    // caps, which is the `not-negotiated`/`No RTP format` error). Link the FIRST video RTP pad only.
    let depay_weak = depay.downgrade();
    src.connect_pad_added(move |_src, pad| {
        let Some(depay) = depay_weak.upgrade() else { return };
        let Some(sink_pad) = depay.static_pad("sink") else { return };
        if sink_pad.is_linked() {
            return;
        }
        let is_video = pad
            .current_caps()
            .and_then(|c| c.structure(0).and_then(|st| st.get::<String>("media").ok()))
            .map(|m| m == "video")
            .unwrap_or(true);
        if is_video {
            let _ = pad.link(&sink_pad);
        }
    });

    let start = std::time::Instant::now();
    pipeline
        .set_state(gstreamer::State::Playing)
        .map_err(|e| format!("play: {e}"))?;

    let bus = pipeline.bus().ok_or("no bus")?;
    let deadline = std::time::Duration::from_secs(secs);
    while start.elapsed() < deadline {
        if let Some(msg) = bus.timed_pop(gstreamer::ClockTime::from_mseconds(200)) {
            if let gstreamer::MessageView::Error(e) = msg.view() {
                let _ = pipeline.set_state(gstreamer::State::Null);
                return Err(format!("pipeline error: {} ({:?})", e.error(), e.debug()));
            }
        }
    }

    pipeline.send_event(gstreamer::event::Eos::new());
    for msg in bus.iter_timed(gstreamer::ClockTime::from_seconds(10)) {
        match msg.view() {
            gstreamer::MessageView::Eos(_) => break,
            gstreamer::MessageView::Error(e) => {
                log::warn!("record finalise error: {} ({:?})", e.error(), e.debug());
                break;
            }
            _ => {}
        }
    }
    let _ = pipeline.set_state(gstreamer::State::Null);

    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    if bytes < 1000 {
        return Err(format!("recorded only {bytes} B — camera unreachable or no video"));
    }
    Ok((bytes, start.elapsed().as_secs()))
}

/// Record a camera and upload the clip to R2 via control (card #144) — reuses the same cloud path an
/// appliance uses: POST the manifest to control (box token) → get a presigned PUT → send the bytes
/// straight to R2 (no video crosses the VPS) → confirm. The phone edge is unenrolled, so the box
/// token is passed in (the app holds it from self-register, #143). No tailnet: all outbound HTTPS.
#[cfg(feature = "gst-linked")]
pub fn record_and_upload(
    url: &str,
    secs: u64,
    out: &std::path::Path,
    box_token: &str,
    control_url: &str,
) -> Result<serde_json::Value, String> {
    let (bytes, elapsed) = record_camera(url, secs, out)?;

    let epoch = app_core::timeutil::now_rfc3339();
    let clip_id = format!("phone-{}", out.file_stem().and_then(|s| s.to_str()).unwrap_or("clip"));
    let clip_id = format!("{clip_id}-{}", bytes); // unique-ish per capture without a clock call
    let payload = serde_json::json!({
        "camera_ip": "phone",
        "clip_id": clip_id,
        "friendly_name": clip_id,
        "duration_secs": elapsed as i64,
        "size_bytes": bytes as i64,
        "started_at": epoch,
    });

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(300))
        .build();

    // 1. Manifest → presigned PUT.
    let manifest = agent
        .post(&format!("{control_url}/config/recordings/clip-metadata"))
        .set("Authorization", &format!("Bearer {box_token}"))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|e| format!("manifest push: {e}"))?
        .into_json::<serde_json::Value>()
        .map_err(|e| format!("manifest response: {e}"))?;
    let put_url = manifest
        .get("upload")
        .and_then(|u| u.get("url"))
        .and_then(|u| u.as_str())
        .ok_or("control returned no upload URL (R2 not configured for this tenant?)")?
        .to_string();

    // 2. PUT the bytes to R2. Content-Length set ourselves — an unsized reader makes ureq use
    // chunked encoding, which R2 rejects on a presigned PUT.
    let file = std::fs::File::open(out).map_err(|e| format!("open clip: {e}"))?;
    agent
        .put(&put_url)
        .set("Content-Type", "video/mp4")
        .set("Content-Length", &bytes.to_string())
        .send(file)
        .map_err(|e| format!("R2 PUT: {e}"))?;

    // 3. Confirm the clip is durable so control marks it uploaded.
    let _ = agent
        .post(&format!("{control_url}/config/recordings/{clip_id}/uploaded"))
        .set("Authorization", &format!("Bearer {box_token}"))
        .call();

    Ok(serde_json::json!({ "clip_id": clip_id, "bytes": bytes, "uploaded": true }))
}
