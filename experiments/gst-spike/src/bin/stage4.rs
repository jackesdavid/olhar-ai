//! Stage 4 spike (card #138, G2): can GStreamer produce **fMP4 that a browser `MediaSource` accepts**,
//! live from the camera? That is the one unknown that decides whether the go2rtc `/api/ws` MSE
//! contract can be served by us — everything else in Stage 4 (the WebSocket, the handshake) is
//! plumbing we control.
//!
//!   CAM_RTSP='rtsp://user:pass@host/streamN' cargo run --bin stage4
//!
//! For MSE the browser needs: an **init segment** (`ftyp` + `moov`) first, then **media segments**
//! (`moof` + `mdat`). This dumps the top-level box types of each buffer so we can see exactly that,
//! and prints the `codecs=` string we would have to advertise in the handshake reply.
//!
//! **VERDICT (Mac, 2026-07-24): yes.** 32 live segments off the real camera; buffer 0 is `ftyp+moov`
//! (HEADER-flagged), the rest is the continuous fMP4 byte stream. Concatenated it walks
//! `ftyp+moov` + `styp+moof+mdat`xN intact and `ffprobe`s clean.
//!
//! **`h264timestamper` is load-bearing — do not drop it.** `cmafmux` rejects buffers with no DTS and
//! `rtph264depay` supplies only PTS. Without it the pipeline goes EOS in ~0.1s having emitted nothing,
//! and the only bus message blames `rtspsrc` (`streaming stopped, reason error (-5)`) — the true cause
//! (`Require DTS for video streams`) is visible ONLY under `GST_DEBUG=fmp4mux:6`. In the edge itself,
//! prefer a Rust pad probe setting `DTS = PTS` (cameras have no B-frames) so Android/Jetson need one
//! plugin less. `cmafmux` also refuses `byte-stream`, hence the `stream-format=avc` capsfilter.
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Top-level ISO-BMFF box types in a buffer (`[u32 size][4cc]`, repeated).
fn box_types(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 8 <= data.len() {
        let size = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let ty = String::from_utf8_lossy(&data[i + 4..i + 8]).to_string();
        out.push(ty);
        if size < 8 {
            break; // 0 or 1 mean "to EOF" / 64-bit — stop walking rather than guess
        }
        i += size;
    }
    out
}

/// The `codecs=` string MSE needs, read from the `avcC` box in the init segment:
/// `avc1.` + profile/compat/level as hex (`01 64 00 16` -> `avc1.640016`).
fn codecs_from_init(init: &[u8]) -> Option<String> {
    let pos = init.windows(4).position(|w| w == b"avcC")?;
    let b = init.get(pos + 4..pos + 8)?; // configurationVersion, profile, compat, level
    Some(format!("avc1.{:02x}{:02x}{:02x}", b[1], b[2], b[3]))
}

fn main() -> Result<()> {
    gst::init().context("gst init")?;
    let url = std::env::var("CAM_RTSP").context("set CAM_RTSP")?;
    let want: u64 = std::env::var("SEGMENTS").ok().and_then(|s| s.parse().ok()).unwrap_or(6);

    // cmafmux emits a CMAF init segment followed by fragments — the exact shape MSE wants.
    let desc = format!(
        "rtspsrc location=\"{url}\" protocols=tcp latency=100 ! rtph264depay ! h264parse ! h264timestamper \
         ! video/x-h264,stream-format=avc,alignment=au \
         ! cmafmux fragment-duration=1000000000 \
         ! appsink name=out sync=false"
    );
    println!("[stage4] pipeline: rtspsrc ! rtph264depay ! h264parse ! h264timestamper ! caps(avc/au) ! cmafmux ! appsink");

    let pipeline = gst::parse::launch(&desc)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
    let appsink = pipeline
        .by_name("out")
        .context("appsink")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| anyhow::anyhow!("not appsink"))?;

    let n = Arc::new(AtomicU64::new(0));
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let caps_seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let init: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let (n, log, caps_seen, init) = (n.clone(), log.clone(), caps_seen.clone(), init.clone());
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = match sink.pull_sample() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("[stage4] pull_sample failed: {e:?}");
                            return Err(gst::FlowError::Eos);
                        }
                    };
                    // fmp4mux pushes ONE BufferList per fragment: the moof buffer followed by the
                    // mdat payload buffers. Concatenating the list is what yields a complete
                    // MSE-appendable segment — a single buffer() is only a slice of it.
                    let mut seg: Vec<u8> = Vec::new();
                    let mut parts = 0usize;
                    let mut header = false;
                    if let Some(list) = sample.buffer_list() {
                        for buf in list.iter() {
                            if buf.flags().contains(gst::BufferFlags::HEADER) {
                                header = true;
                            }
                            if let Ok(map) = buf.map_readable() {
                                seg.extend_from_slice(map.as_slice());
                                parts += 1;
                            }
                        }
                    } else if let Some(buf) = sample.buffer() {
                        header = buf.flags().contains(gst::BufferFlags::HEADER);
                        if let Ok(map) = buf.map_readable() {
                            seg.extend_from_slice(map.as_slice());
                            parts = 1;
                        }
                    }
                    if !seg.is_empty() {
                        let i = n.fetch_add(1, Ordering::Relaxed);
                        if i == 0 {
                            *init.lock().unwrap() = seg.clone();
                            if let Some(c) = sample.caps() {
                                *caps_seen.lock().unwrap() = Some(c.to_string());
                            }
                        }
                        let boxes = box_types(&seg).join("+");
                        log.lock().unwrap().push(format!(
                            "  seg {i:>2}: {:>7} B  ({parts} buffers)  boxes=[{boxes}]{}",
                            seg.len(),
                            if header { "  <-- HEADER (init segment)" } else { "" }
                        ));
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline.bus().context("bus")?;
    let mut ticks = 0;
    loop {
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) {
            if let gst::MessageView::Error(e) = msg.view() {
                pipeline.set_state(gst::State::Null).ok();
                anyhow::bail!("pipeline error: {} ({:?})", e.error(), e.debug());
            }
        }
        if n.load(Ordering::Relaxed) >= want {
            break;
        }
        ticks += 1;
        if ticks > 200 {
            pipeline.set_state(gst::State::Null).ok();
            anyhow::bail!("timeout: only {} segments in ~40s", n.load(Ordering::Relaxed));
        }
    }
    pipeline.set_state(gst::State::Null).ok();

    for line in log.lock().unwrap().iter() {
        println!("{line}");
    }
    let caps = caps_seen.lock().unwrap().clone().unwrap_or_default();
    println!("[stage4] appsink caps: {caps}");
    println!(
        "[stage4] ✅ GStreamer produced {} fMP4 segments live from the camera",
        n.load(Ordering::Relaxed)
    );
    match codecs_from_init(&init.lock().unwrap()) {
        Some(c) => println!("[stage4] handshake would advertise codecs=\"{c}\""),
        None => println!("[stage4] ⚠️  no avcC in the init segment — cannot derive codecs="),
    }
    println!("[stage4] MSE needs the first buffer to carry ftyp+moov and the rest moof+mdat — see above.");
    Ok(())
}
