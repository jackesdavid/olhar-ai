//! Stage 0 spike for the GStreamer migration (card #138).
//!
//! Proves, from linked `gstreamer-rs` (NOT a `gst-launch` subprocess), that on this Mac we can:
//!   1. init GStreamer and build a pipeline programmatically,
//!   2. run **hardware decode** via `vtdec` (VideoToolbox), and
//!   3. pull decoded frames back into Rust via `appsink`.
//!
//! That triad is exactly what Stage 1 (live RTSP decode → motion/detector) needs. We synthesise the
//! H264 here (`videotestsrc ! x264enc`) so the spike needs no camera; Stage 1 swaps the head for
//! `rtspsrc`. The same source must build on the Jetson with `nvv4l2decoder` before Stage 0 is "done".
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn main() -> Result<()> {
    gst::init().context("gst::init")?;
    let version = gst::version_string();
    println!("[spike] GStreamer runtime: {version}");

    // Encode a synthetic 720p clip, then HW-decode it and hand frames to Rust. If `vtdec` is not
    // available the pipeline build fails loudly — that IS the signal we want on a box without HW.
    let n_buffers = 120;
    // Stage 0 proves the LINKED gstreamer-rs path with REAL HW decode (vtdec / VideoToolbox).
    // FIELD FINDING + FIX (Mac, GStreamer 1.28.5, 2026-07-24): forcing an explicit `profile=main`
    // capsfilter on x264enc's output makes `vtdec` HW-decode all frames. Without it vtdec errors
    // "no valid frames" — x264enc's DEFAULT negotiation produces a profile VideoToolbox HW rejects.
    // This is a synthetic-stream artifact: real camera H264 always declares a standard profile in
    // its SPS, so `rtspsrc`-fed vtdec (Stage 1) should not hit this. HW decode WORKS on this Mac.
    let desc = format!(
        "videotestsrc num-buffers={n_buffers} ! video/x-raw,width=1280,height=720,framerate=30/1 \
         ! x264enc tune=zerolatency key-int-max=30 ! video/x-h264,profile=main \
         ! h264parse config-interval=-1 ! vtdec ! videoconvert ! video/x-raw,format=NV12 \
         ! appsink name=out sync=false"
    );
    println!("[spike] pipeline: {desc}");

    let pipeline = gst::parse::launch(&desc)
        .context("parse::launch — a missing element (vtdec/x264enc/appsink) fails here")?
        .downcast::<gst::Pipeline>()
        .expect("a launch of a bin is a Pipeline");

    let appsink = pipeline
        .by_name("out")
        .context("appsink 'out' not found")?
        .downcast::<gst_app::AppSink>()
        .expect("named 'out' is the appsink");

    // Count frames Rust actually received, and capture the negotiated caps once — proof the decoded
    // buffers really reached us (not just that the pipeline ran).
    let frames = Arc::new(AtomicU64::new(0));
    let caps_seen: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    {
        let frames = frames.clone();
        let caps_seen = caps_seen.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    if let Some(buf) = sample.buffer() {
                        let n = frames.fetch_add(1, Ordering::Relaxed);
                        if n == 0 {
                            if let Some(caps) = sample.caps() {
                                *caps_seen.lock().unwrap() = Some(caps.to_string());
                            }
                            println!("[spike] first decoded frame: {} bytes", buf.size());
                        }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    pipeline.set_state(gst::State::Playing).context("set Playing")?;

    // Drive the bus until EOS or error, so a decode failure surfaces instead of hanging.
    let bus = pipeline.bus().context("pipeline bus")?;
    for msg in bus.iter_timed(gst::ClockTime::from_seconds(20)) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(_) => break,
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).ok();
                anyhow::bail!(
                    "pipeline error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
            }
            _ => {}
        }
    }

    pipeline.set_state(gst::State::Null).context("set Null")?;

    let got = frames.load(Ordering::Relaxed);
    let caps = caps_seen.lock().unwrap().clone().unwrap_or_default();
    println!("[spike] decoded frames delivered to Rust: {got}/{n_buffers}");
    println!("[spike] negotiated appsink caps: {caps}");

    anyhow::ensure!(got >= n_buffers as u64 - 2, "too few frames — HW decode/appsink path broken");
    println!("[spike] ✅ STAGE 0 PASS on this box: linked gstreamer-rs + vtdec HW decode (VideoToolbox, profile=main) + appsink frame pull");
    Ok(())
}
