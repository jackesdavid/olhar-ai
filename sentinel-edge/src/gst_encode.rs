//! JPEG → H264 for a device camera's recording ring (#175).
//!
//! A wall camera hands the edge H264 and everything downstream assumes it: the pre-roll ring stores
//! Annex-B access units, and the muxer remuxes them without re-encoding. A device camera hands the
//! edge JPEG, because that is what the frame hub wants — so recording had nothing to store and
//! finalised clips at 587 bytes, a container with no samples.
//!
//! This bridges the two. It is the ONLY place a frame is re-encoded, and only for cameras that have
//! no H264 of their own; a wall camera's bytes still reach the file untouched.
//!
//! **Hardware first.** On the phone `vtenc_h264` is VideoToolbox, so the encode runs on the media
//! engine rather than the CPU that is already doing MOG2 and YuNet on the same frames. The software
//! encoders are a fallback for a platform whose GStreamer build lacks it, not a preference —
//! `x264enc` on a phone would compete with the detection path for the cores that make it work.
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Encoders tried in order. The first that exists in this platform's GStreamer build is used.
///
/// `vtenc_h264` is hardware (VideoToolbox) and is present in the iOS and macOS frameworks;
/// `x264enc` and `openh264enc` are software, and `avenc_h264` (libav) is the last resort. A build
/// with none of them cannot record a device camera, and says so rather than producing an empty clip.
const ENCODERS: &[&str] = &["vtenc_h264", "x264enc", "openh264enc", "avenc_h264"];

/// Per-encoder properties that matter. `realtime`/`zerolatency` stop the encoder buffering frames
/// waiting for lookahead — on a 5-frame clip that is the difference between a file and nothing.
fn encoder_props(name: &str) -> &'static str {
    match name {
        "vtenc_h264" => " realtime=true allow-frame-reordering=false",
        "x264enc" => " tune=zerolatency speed-preset=veryfast key-int-max=30",
        "openh264enc" => " complexity=low",
        _ => "",
    }
}

fn first_available_encoder() -> Option<&'static str> {
    let registry = gst::Registry::get();
    ENCODERS
        .iter()
        .copied()
        .find(|n| registry.find_feature(n, gst::ElementFactory::static_type()).is_some())
}

/// One access unit out of the encoder: Annex-B bytes plus the instant the SOURCE frame was
/// captured.
///
/// The capture instant travels with the frame rather than being taken when the encoder emits it.
/// The mux assigns PTS from this, so using the emit time would fold the encoder's own latency into
/// the timeline — a clip that plays at the wrong speed and reports the wrong duration, which is
/// precisely what `gst_mux` exists to avoid.
pub struct EncodedFrame {
    pub bytes: Vec<u8>,
    pub at: Instant,
}

/// A running JPEG→H264 encoder for one camera.
pub struct JpegEncoder {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    /// Capture instants of frames pushed in but not yet emitted, oldest first. The encoder may hold
    /// a frame or emit several at once, so the queue pairs each output with the right input time.
    pending: Arc<Mutex<std::collections::VecDeque<Instant>>>,
}

impl JpegEncoder {
    /// Build and start the encoder. `on_frame` is called for each access unit, on the appsink's
    /// thread — keep it cheap; the intended body is "push into the mem ring".
    pub fn start(
        camera_ip: &str,
        on_frame: impl Fn(EncodedFrame) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        crate::media::init_gstreamer()?;
        let enc = first_available_encoder().ok_or_else(|| {
            format!(
                "no H264 encoder in this GStreamer build (tried {}) — a device camera cannot be \
                 recorded without one",
                ENCODERS.join(", ")
            )
        })?;
        log::info!("jpeg-encoder[{camera_ip}]: using {enc}");

        // `is-live=true` + `do-timestamp=false`: we assign no PTS here at all. The ring stores raw
        // access units and `gst_mux` stamps them from capture time when the clip is written, so a
        // second timeline invented here could only disagree with that one.
        let desc = format!(
            "appsrc name=src is-live=true format=time do-timestamp=false \
             ! jpegdec ! videoconvert ! {enc}{props} \
             ! h264parse config-interval=1 \
             ! video/x-h264,stream-format=byte-stream,alignment=au \
             ! appsink name=out sync=false max-buffers=8 drop=true",
            props = encoder_props(enc)
        );
        let pipeline = gst::parse::launch(&desc)
            .map_err(|e| format!("gst parse (encoder): {e}"))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| "not a pipeline".to_string())?;

        let src = pipeline
            .by_name("src")
            .ok_or("appsrc missing")?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| "not an appsrc".to_string())?;
        src.set_caps(Some(&gst::Caps::builder("image/jpeg").build()));

        let sink = pipeline
            .by_name("out")
            .ok_or("appsink missing")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| "not an appsink".to_string())?;

        let pending: Arc<Mutex<std::collections::VecDeque<Instant>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let pending_cb = Arc::clone(&pending);
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    // Pair with the oldest un-emitted capture time. `Instant::now()` as a fallback
                    // keeps a frame rather than dropping it if the queue ever runs dry.
                    let at = pending_cb
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(Instant::now);
                    on_frame(EncodedFrame {
                        bytes: map.as_slice().to_vec(),
                        at,
                    });
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("encoder pipeline start: {e}"))?;

        Ok(Self {
            pipeline,
            src,
            pending,
        })
    }

    /// Feed one JPEG. Best-effort: a dropped frame is one missing frame in a clip, never a reason
    /// to tear the encoder down mid-recording.
    pub fn push(&self, jpeg: &[u8], at: Instant) {
        // Bound the queue. If the encoder stalls, an unbounded queue of Instants would grow without
        // limit and every later frame would be paired with a badly stale time.
        {
            let mut q = self.pending.lock().unwrap();
            if q.len() > 120 {
                q.pop_front();
            }
            q.push_back(at);
        }
        let buffer = gst::Buffer::from_slice(jpeg.to_vec());
        if let Err(e) = self.src.push_buffer(buffer) {
            log::debug!("jpeg-encoder: push failed: {e}");
        }
    }
}

impl Drop for JpegEncoder {
    fn drop(&mut self) {
        let _ = self.src.end_of_stream();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
