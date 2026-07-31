//!
//! **Why this exists only now.** The earlier attempt (Phase 2b) used a `gst-launch` subprocess:
//! `fdsrc ! h264parse ! mp4mux` and it failed with *"Buffer has no PTS"*, because `h264parse` cannot
//! invent timing for a raw Annex-B byte stream whose SPS carries no VUI timing info. The documented
//! fix was `appsrc` with **manually-assigned buffer PTS**, which needs linked bindings — impossible
//! while the edge linked nothing. The phone-as-edge pivot made linked `gstreamer-rs` the design, so
//! that fix is now available and this module implements it.
//!
//! **PTS comes from real capture time**, not a nominal framerate: `H264Frame.at` is the instant the
//! frame was captured, so the first frame anchors the origin and every later PTS is its offset from
//! that. Deriving PTS from an assumed fps would produce clips that *look* fine but play at the wrong
//! speed and report the wrong duration — the failure mode this whole path has to avoid.
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::time::Instant;

/// Owns the running pipeline; `wait()` finalises the file (mp4mux writes the moov atom).
pub struct GstMuxer {
    pipeline: gst::Pipeline,
}

/// The write end handed to the relay thread — one buffer per frame, PTS assigned here.
pub struct GstSink {
    src: gst_app::AppSrc,
    /// Set from the first frame written; every PTS is measured against it.
    origin: Option<Instant>,
}

/// Build and start `appsrc ! h264parse ! mp4mux ! filesink` for `output`.
pub fn start(output: &std::path::Path) -> Result<(GstMuxer, GstSink), String> {
    crate::media::init_gstreamer()?;
    let path = output.to_string_lossy();
    // `format=time` is what lets our per-buffer PTS be honoured; faststart puts the moov up front so
    let desc = format!(
        "appsrc name=src is-live=false format=time \
         ! h264parse ! mp4mux faststart=true ! filesink location=\"{path}\""
    );
    let pipeline = gst::parse::launch(&desc)
        .map_err(|e| format!("gst parse: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "not a pipeline".to_string())?;
    let src = pipeline
        .by_name("src")
        .ok_or("appsrc missing")?
        .downcast::<gst_app::AppSrc>()
        .map_err(|_| "not an appsrc".to_string())?;

    // Tell h264parse exactly what it's being fed — Annex-B, one access unit per buffer — so it never
    // has to guess framing (the guessing is what failed before).
    src.set_caps(Some(
        &gst::Caps::builder("video/x-h264")
            .field("stream-format", "byte-stream")
            .field("alignment", "au")
            .build(),
    ));

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| format!("gst play: {e}"))?;
    Ok((GstMuxer { pipeline }, GstSink { src, origin: None }))
}

impl GstSink {
    /// Push one complete access unit with its real capture time as PTS.
    pub fn write_frame(&mut self, data: &[u8], at: Instant) -> std::io::Result<()> {
        let origin = *self.origin.get_or_insert(at);
        let pts = at.saturating_duration_since(origin);
        let mut buf = gst::Buffer::from_slice(data.to_vec());
        {
            let b = buf.get_mut().expect("fresh buffer is uniquely owned");
            b.set_pts(gst::ClockTime::from_nseconds(pts.as_nanos() as u64));
        }
        self.src
            .push_buffer(buf)
            .map(|_| ())
            .map_err(|e| std::io::Error::other(format!("appsrc push: {e:?}")))
    }

    pub fn finish(self) {
        let _ = self.src.end_of_stream();
    }
}

impl GstMuxer {
    /// Block until the muxer has written the file out, then tear the pipeline down.
    /// Bounded so a wedged pipeline can never hang `stop()` forever.
    pub fn wait(&mut self) {
        if let Some(bus) = self.pipeline.bus() {
            for msg in bus.iter_timed(gst::ClockTime::from_seconds(15)) {
                match msg.view() {
                    gst::MessageView::Eos(_) => break,
                    gst::MessageView::Error(e) => {
                        log::warn!("gst muxer error: {} ({:?})", e.error(), e.debug());
                        break;
                    }
                    _ => {}
                }
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
