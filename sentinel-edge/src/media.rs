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
}

impl Backend {
    /// Resolve the config string, falling back to `Ffmpeg` for anything unrecognized AND for
    /// `"gstreamer"` on a box that doesn't actually have `gst-launch-1.0` on PATH — a stale or
    /// mistaken flag must never leave an operation with no working tool
    /// (`state::gstreamer_ok()` mirrors `ffmpeg_ok()`).
    pub fn resolve(config_value: &str) -> Self {
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
        }
    }
}
