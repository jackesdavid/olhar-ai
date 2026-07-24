//! Stage 1 spike (card #138) — the real A1 path: live **RTSP from a real camera** → **HW decode**
//! (`vtdec` / VideoToolbox) → frames pulled into Rust via `appsink`. This is what the edge's
//! motion/detector will consume, replacing ffmpeg's MJPEG-on-pipe decode.
//!
//! The RTSP URL (with credentials) is read from `CAM_RTSP` so the password never lands in git.
//!   CAM_RTSP='rtsp://user:pass@host:554/streamN' cargo run --bin stage1
//!
//! Unlike the Stage 0 synthetic stream, a real camera declares a standard H264 profile in its SPS,
//! so `vtdec` should decode directly with no profile-forcing needed — this run confirms that.
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    gst::init().context("gst::init")?;
    let url = std::env::var("CAM_RTSP").context("set CAM_RTSP='rtsp://user:pass@host/stream'")?;
    let target_frames: u64 = std::env::var("FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
    println!("[stage1] GStreamer {}, pulling {target_frames} frames from the camera", gst::version_string());

    // rtspsrc has sometimes-pads (linked at runtime); gst::parse handles the delayed link. vtdec
    // (HW) sits right after h264parse. No profile-forcing — the camera's SPS carries a real profile.
    let desc = format!(
        "rtspsrc location=\"{url}\" latency=100 protocols=tcp ! rtph264depay ! h264parse \
         ! vtdec ! videoconvert ! video/x-raw,format=NV12 ! appsink name=out sync=false max-buffers=2 drop=true"
    );
    // Print the pipeline WITHOUT the credentials.
    println!("[stage1] pipeline: rtspsrc (camera) ! rtph264depay ! h264parse ! vtdec ! videoconvert ! appsink");

    let pipeline = gst::parse::launch(&desc)
        .context("parse::launch")?
        .downcast::<gst::Pipeline>()
        .expect("Pipeline");

    let appsink = pipeline
        .by_name("out")
        .context("appsink")?
        .downcast::<gst_app::AppSink>()
        .expect("AppSink");

    let frames = Arc::new(AtomicU64::new(0));
    let res: Arc<Mutex<Option<(i32, i32)>>> = Arc::new(Mutex::new(None));
    {
        let frames = frames.clone();
        let res = res.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let n = frames.fetch_add(1, Ordering::Relaxed);
                    if n == 0 {
                        if let Some(caps) = sample.caps() {
                            if let Some(s) = caps.structure(0) {
                                let w = s.get::<i32>("width").unwrap_or(0);
                                let h = s.get::<i32>("height").unwrap_or(0);
                                *res.lock().unwrap() = Some((w, h));
                                println!("[stage1] first decoded frame: {w}x{h}");
                            }
                        }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    pipeline.set_state(gst::State::Playing).context("Playing")?;

    // Live source never sends EOS — run until we've pulled enough frames or a bounded timeout.
    let bus = pipeline.bus().context("bus")?;
    let mut ticks = 0;
    loop {
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) {
            if let gst::MessageView::Error(err) = msg.view() {
                pipeline.set_state(gst::State::Null).ok();
                anyhow::bail!("pipeline error: {} ({:?})", err.error(), err.debug());
            }
        }
        if frames.load(Ordering::Relaxed) >= target_frames {
            break;
        }
        ticks += 1;
        if ticks > 150 {
            // ~30s ceiling — connect/decode never got going.
            pipeline.set_state(gst::State::Null).ok();
            anyhow::bail!("timeout: only {} frames in ~30s", frames.load(Ordering::Relaxed));
        }
    }

    pipeline.set_state(gst::State::Null).context("Null")?;
    let got = frames.load(Ordering::Relaxed);
    let (w, h) = res.lock().unwrap().unwrap_or((0, 0));
    println!("[stage1] pulled {got} decoded {w}x{h} frames from the live camera via vtdec HW decode");
    println!("[stage1] ✅ STAGE 1 (Mac): real RTSP → vtdec HW → appsink works — the A1 path is proven on a real camera");
    Ok(())
}
