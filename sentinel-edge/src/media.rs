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

/// The 13 plugins `capture.rs`/`gst_mse.rs` instantiate, registered from their statically-linked
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
        });
    }
}
