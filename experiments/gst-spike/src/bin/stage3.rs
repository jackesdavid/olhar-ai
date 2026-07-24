//! Stage 3 diagnosis (card #138, A2): replicate `sentinel-edge/src/gst_mux.rs`'s EXACT pipeline
//! outside the edge, feeding it real camera H264, to find out why the muxed clip came out 0 bytes.
//!
//!   H264_FILE=/tmp/cam.h264 OUT=/tmp/out.mp4 cargo run --bin stage3
//!
//! Splits the Annex-B stream into access units (new AU at each VCL slice, carrying any preceding
//! SPS/PPS with it), pushes one buffer per AU with an increasing PTS, then EOS — exactly what the
//! edge's relay does with real frame timestamps.
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

/// Split Annex-B into access units. A new AU starts at a VCL NAL (type 1 or 5); SPS/PPS/AUD that
/// precede it belong to that AU.
fn split_access_units(data: &[u8]) -> Vec<Vec<u8>> {
    // Collect (offset, nal_type) for every start code.
    let mut marks: Vec<(usize, u8)> = Vec::new();
    let mut i = 0;
    while i + 4 < data.len() {
        let sc4 = data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 0 && data[i + 3] == 1;
        let sc3 = data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1;
        if sc4 || sc3 {
            let hdr = if sc4 { i + 4 } else { i + 3 };
            if hdr < data.len() {
                marks.push((i, data[hdr] & 0x1F));
            }
            i = hdr;
        } else {
            i += 1;
        }
    }
    // Cut before each VCL NAL that follows a previous VCL NAL (i.e. one AU per picture).
    let mut cuts: Vec<usize> = Vec::new();
    let mut seen_vcl = false;
    for (off, t) in &marks {
        let is_vcl = *t == 1 || *t == 5;
        if is_vcl {
            if seen_vcl {
                cuts.push(*off);
            }
            seen_vcl = true;
        } else if seen_vcl && (*t == 7 || *t == 8 || *t == 9) {
            // SPS/PPS/AUD after a picture → start of the next AU.
            cuts.push(*off);
            seen_vcl = false;
        }
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    for c in cuts {
        if c > start {
            out.push(data[start..c].to_vec());
        }
        start = c;
    }
    if start < data.len() {
        out.push(data[start..].to_vec());
    }
    out
}

fn main() -> Result<()> {
    gst::init().context("gst init")?;
    let path = std::env::var("H264_FILE").unwrap_or_else(|_| "/tmp/cam.h264".into());
    let out = std::env::var("OUT").unwrap_or_else(|_| "/tmp/out.mp4".into());
    let data = std::fs::read(&path).with_context(|| format!("read {path}"))?;
    let aus = split_access_units(&data);
    println!("[stage3] {} bytes -> {} access units", data.len(), aus.len());
    anyhow::ensure!(!aus.is_empty(), "no access units parsed");

    // EXACTLY the edge's muxer pipeline.
    let desc = format!(
        "appsrc name=src is-live=false format=time \
         ! h264parse ! mp4mux faststart=true ! filesink location=\"{out}\""
    );
    println!("[stage3] pipeline: {desc}");
    let pipeline = gst::parse::launch(&desc)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
    let src = pipeline
        .by_name("src")
        .context("appsrc")?
        .downcast::<gst_app::AppSrc>()
        .map_err(|_| anyhow::anyhow!("not appsrc"))?;
    src.set_caps(Some(
        &gst::Caps::builder("video/x-h264")
            .field("stream-format", "byte-stream")
            .field("alignment", "au")
            .build(),
    ));

    pipeline.set_state(gst::State::Playing)?;

    // 15fps nominal for the test (the edge uses real capture instants).
    let step = gst::ClockTime::from_mseconds(1000 / 15);
    let mut pushed = 0u64;
    for (i, au) in aus.iter().enumerate() {
        let mut buf = gst::Buffer::from_slice(au.clone());
        {
            let b = buf.get_mut().unwrap();
            b.set_pts(step * (i as u64));
        }
        match src.push_buffer(buf) {
            Ok(_) => pushed += 1,
            Err(e) => {
                println!("[stage3] push failed at AU {i}: {e:?}");
                break;
            }
        }
    }
    println!("[stage3] pushed {pushed} buffers, sending EOS");
    let _ = src.end_of_stream();

    // Wait for EOS/error so mp4mux can write the trailer.
    let bus = pipeline.bus().context("bus")?;
    let mut verdict = "no EOS within 15s".to_string();
    for msg in bus.iter_timed(gst::ClockTime::from_seconds(15)) {
        match msg.view() {
            gst::MessageView::Eos(_) => {
                verdict = "EOS".into();
                break;
            }
            gst::MessageView::Error(e) => {
                verdict = format!("ERROR from {:?}: {} ({:?})",
                    e.src().map(|s| s.path_string()), e.error(), e.debug());
                break;
            }
            _ => {}
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    println!("[stage3] bus verdict: {verdict}");

    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!("[stage3] output {out}: {size} bytes");
    anyhow::ensure!(size > 1000, "MUXER PRODUCED NOTHING — this reproduces the edge failure");
    println!("[stage3] ✅ muxer works standalone");
    Ok(())
}
