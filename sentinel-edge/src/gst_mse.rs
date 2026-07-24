//! Live **fMP4 over WebSocket** (card #138, touchpoint G2) — the go2rtc `/api/ws` replacement.
//!
//! This is the piece that lets the edge serve live video itself. It keeps go2rtc's wire contract
//! **byte for byte**, so the gateway proxy, the web player and the mobile WebView player are all
//! untouched: client sends `{"type":"mse","value":"<codecs it supports>"}`, we answer
//! `{"type":"mse","value":"video/mp4; codecs=\"…\""}` (fed straight to `addSourceBuffer`), then the
//! socket carries nothing but fMP4 bytes.
//!
//! **Where the H264 comes from.** For now, the same place [`crate::capture`] and
//! [`crate::recording`] already take it: go2rtc's local RTSP re-serve on `127.0.0.1:8554`. So the
//! `src=` query parameter *is* the upstream stream name and no lookup is needed. That is
//! deliberate and temporary — when go2rtc goes away (touchpoint G5) the edge becomes the fan-out
//! point itself: one `rtspsrc` per camera, `tee`'d to decode + record + this. Doing it that way
//! now would mean rewriting two working paths before this one is proven.
//!
//! **`cmafmux` requires DTS and RTP depayloading supplies only PTS.** Left unset, the pipeline goes
//! EOS within ~0.1s having produced *nothing*, and the only message on the bus blames `rtspsrc`
//! (`streaming stopped, reason error (-5)`) — the real cause (`Require DTS for video streams`) is
//! visible only under `GST_DEBUG=fmp4mux:6`. `h264timestamper` fixes it but lives in
//! gst-plugins-bad; a pad probe copying PTS→DTS costs us nothing and keeps one more plugin off the
//! Android and Jetson builds. Our cameras carry no B-frames, so the two are identical anyway.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::broadcast;

/// How much video each fragment holds. Small fragments mean low latency (the browser cannot play
/// what the muxer has not closed yet) at the cost of a little per-fragment overhead. 500ms matches
/// what go2rtc gives us today.
const FRAGMENT_NS: u64 = 500_000_000;

/// A running per-stream muxer, shared by every viewer of that camera.
pub struct MseStream {
    /// `ftyp` + `moov` — every newly-attached viewer needs this before any fragment.
    pub init: Vec<u8>,
    /// The MIME the client must pass to `addSourceBuffer`, e.g. `video/mp4; codecs="avc1.640016"`.
    pub mime: String,
    /// Live fragments. Viewers that fall behind are dropped from the tail rather than stalling the
    /// muxer — a late fragment is worthless for live video.
    tx: broadcast::Sender<Arc<Vec<u8>>>,
    pipeline: gst::Pipeline,
}

impl MseStream {
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<u8>>> {
        self.tx.subscribe()
    }

    /// Viewers currently attached. The registry uses this to tear an idle stream down.
    pub fn viewers(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Drop for MseStream {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn registry() -> &'static Mutex<HashMap<String, Arc<MseStream>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<MseStream>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The `codecs=` string MSE needs, read from the `avcC` box in the init segment:
/// `avc1.` + profile / constraint flags / level as hex (`01 64 00 16` → `avc1.640016`).
fn codecs_from_init(init: &[u8]) -> Option<String> {
    let pos = init.windows(4).position(|w| w == b"avcC")?;
    let b = init.get(pos + 4..pos + 8)?; // configurationVersion, profile, compat, level
    Some(format!("avc1.{:02x}{:02x}{:02x}", b[1], b[2], b[3]))
}

/// Get the stream for `name`, starting the pipeline if nobody is watching it yet.
///
/// Blocks until the init segment has arrived, because a viewer cannot be served without it — that
/// is one keyframe away, so bounded at 10s to fail a dead camera fast rather than hang the socket.
pub fn attach(name: &str) -> Result<Arc<MseStream>, String> {
    if let Some(existing) = registry().lock().unwrap().get(name) {
        return Ok(Arc::clone(existing));
    }
    let started = start(name)?;
    let mut reg = registry().lock().unwrap();
    // Another request may have won the race while we were building; prefer the one already published
    // so both viewers share a single pipeline.
    let stream = Arc::clone(reg.entry(name.to_string()).or_insert(started));
    Ok(stream)
}

/// Drop a stream once its last viewer has gone, so an unwatched camera costs nothing.
pub fn detach(name: &str) {
    let mut reg = registry().lock().unwrap();
    if reg.get(name).is_some_and(|s| s.viewers() == 0) {
        reg.remove(name);
        log::info!("mse: {name} has no viewers left — pipeline stopped");
    }
}

fn start(name: &str) -> Result<Arc<MseStream>, String> {
    gst::init().map_err(|e| format!("gst init: {e}"))?;
    // go2rtc's local re-serve; the contract's `src=` is already the stream name there.
    let url = format!("rtsp://127.0.0.1:8554/{name}");
    let desc = format!(
        "rtspsrc location=\"{url}\" protocols=tcp latency=100 \
         ! rtph264depay ! h264parse name=parse \
         ! video/x-h264,stream-format=avc,alignment=au \
         ! cmafmux fragment-duration={FRAGMENT_NS} \
         ! appsink name=out sync=false"
    );
    let pipeline = gst::parse::launch(&desc)
        .map_err(|e| format!("gst parse: {e}"))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "not a pipeline".to_string())?;

    // PTS → DTS, without which cmafmux silently produces nothing (see the module note).
    let parse_src = pipeline
        .by_name("parse")
        .ok_or("h264parse missing")?
        .static_pad("src")
        .ok_or("h264parse has no src pad")?;
    parse_src.add_probe(gst::PadProbeType::BUFFER, |_, info| {
        if let Some(gst::PadProbeData::Buffer(buffer)) = &mut info.data {
            if buffer.dts().is_none() {
                let pts = buffer.pts();
                buffer.make_mut().set_dts(pts);
            }
        }
        gst::PadProbeReturn::Ok
    });

    let appsink = pipeline
        .by_name("out")
        .ok_or("appsink missing")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| "not an appsink".to_string())?;

    // 32 fragments ≈ 16s of slack: enough to ride out a stalled viewer, small enough that a viewer
    // that fell that far behind is better off being dropped and resynced.
    let (tx, _) = broadcast::channel::<Arc<Vec<u8>>>(32);
    let init_slot: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    {
        let (tx, init_slot) = (tx.clone(), Arc::clone(&init_slot));
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let Some(buf) = sample.buffer() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let Ok(map) = buf.map_readable() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let bytes = map.as_slice();
                    // The very first buffer is the init segment (`ftyp` + `moov`); everything after
                    // it is the live byte stream.
                    let mut slot = init_slot.lock().unwrap();
                    if slot.is_none() {
                        *slot = Some(bytes.to_vec());
                    } else {
                        let _ = tx.send(Arc::new(bytes.to_vec()));
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| format!("gst play: {e}"))?;

    // Wait for the init segment — it arrives with the first keyframe.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let init = loop {
        if let Some(init) = init_slot.lock().unwrap().clone() {
            break init;
        }
        if std::time::Instant::now() >= deadline {
            let _ = pipeline.set_state(gst::State::Null);
            return Err(format!("{name}: no init segment within 10s"));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };

    let codecs =
        codecs_from_init(&init).ok_or_else(|| format!("{name}: init segment carries no avcC"))?;
    let mime = format!("video/mp4; codecs=\"{codecs}\"");
    log::info!("mse: {name} started ({} B init, {mime})", init.len());
    Ok(Arc::new(MseStream {
        init,
        mime,
        tx,
        pipeline,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `codecs=` string is what decides whether the browser will play the stream at all, and it
    /// is derived by byte-offset into a box we do not control — worth pinning.
    #[test]
    fn reads_the_codec_string_out_of_avcc() {
        let mut init = b"\x00\x00\x00\x08ftypfake\x00\x00\x00\x0eavcC".to_vec();
        init.extend_from_slice(&[0x01, 0x64, 0x00, 0x16]); // High profile, level 2.2
        assert_eq!(codecs_from_init(&init).as_deref(), Some("avc1.640016"));
    }

    #[test]
    fn no_avcc_means_no_codec_string() {
        assert_eq!(codecs_from_init(b"\x00\x00\x00\x08ftypfake"), None);
    }
}

#[cfg(all(test, feature = "gst-linked"))]
mod gst_mse_it {
    //! Integration test for G2 against a live camera served by the local go2rtc RTSP re-serve.
    //! Exercises the real public API — `attach` → `subscribe` → `detach`. Environment-specific,
    //! so `#[ignore]`d:
    //!   GST_IT_CAM=cam_192_168_0_7 cargo test --features gst-linked -- --ignored gst_mse
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "needs a live camera + local go2rtc re-serve"]
    fn serves_an_init_segment_and_live_fragments() {
        let name = std::env::var("GST_IT_CAM").expect("set GST_IT_CAM to a go2rtc stream name");
        let stream = super::attach(&name).expect("pipeline should start");

        // The init segment is what a viewer needs before anything else: ftyp + moov, with an avcC
        // the MIME was derived from.
        assert!(
            stream.init.starts_with(b"\x00\x00\x00\x20ftyp") || stream.init[4..8] == *b"ftyp",
            "init segment must open with an ftyp box, got {:02x?}",
            &stream.init[..16.min(stream.init.len())]
        );
        assert!(
            stream.init.windows(4).any(|w| w == b"moov"),
            "init segment must carry moov"
        );
        assert!(
            stream.mime.starts_with("video/mp4; codecs=\"avc1."),
            "unexpected mime: {}",
            stream.mime
        );

        // Then live fragments, each a self-contained styp/moof+mdat the browser can append.
        let mut rx = stream.subscribe();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut got = 0;
        while got < 3 && Instant::now() < deadline {
            if let Ok(frag) = rx.blocking_recv() {
                assert!(!frag.is_empty(), "fragment must not be empty");
                got += 1;
            }
        }
        assert!(got >= 3, "expected 3 live fragments within 15s, got {got}");

        drop(rx);
        super::detach(&name);
    }
}
