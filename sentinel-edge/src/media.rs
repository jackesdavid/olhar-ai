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
