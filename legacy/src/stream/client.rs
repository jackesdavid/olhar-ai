//! Spawn ffmpeg to decode an RTSP stream into raw grayscale frames, read on a
//! background thread and forwarded to the render loop. ffmpeg's own `-reconnect`
//! flags don't apply to RTSP, so reconnect is driven at the app level by
//! respawning via [`spawn_ffmpeg`]; this module just owns one spawn + its reader
//! threads and classifies why a process stopped.
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::GrayImage;

use super::domain::{FrameSize, LiveStream, StreamEnd};
use crate::app::domain::ActivityGuard;
use crate::config::domain::Transport;
use crate::onvif::client::rtsp_url_with_creds;

/// Everything needed to (re)spawn ffmpeg for one feed. Stored on the
/// `LiveStream` so the supervisor can respawn without the app re-supplying it.
#[derive(Clone)]
pub struct SpawnParams {
    pub rtsp: String,
    pub user: String,
    pub pass: String,
    pub size: FrameSize,
    pub fps: u32,
    pub transport: Transport,
}

/// The live ends of one ffmpeg spawn: the child plus the two receive channels.
pub struct StreamChannels {
    pub child: Child,
    pub frames_rx: Receiver<GrayImage>,
    pub ended_rx: Receiver<StreamEnd>,
}

/// Spawn ffmpeg and its reader threads for `p`. The frame thread forwards raw
/// gray frames; the stderr thread continuously drains stderr (so ffmpeg never
/// blocks on a full pipe) and keeps the most recent error line, which is used
/// to classify why the stream stopped.
pub fn spawn_ffmpeg(p: &SpawnParams) -> Result<StreamChannels, String> {
    let url = rtsp_url_with_creds(&p.rtsp, &p.user, &p.pass);
    let scale = format!("scale={}:{}", p.size.w, p.size.h);
    let fps = p.fps.to_string();

    // `-rtsp_transport` is an INPUT option — it must precede `-i`. Auto omits
    // the flag so ffmpeg negotiates UDP→TCP. (We deliberately do NOT set a
    // socket I/O timeout flag here: a frozen/half-open feed is caught instead
    // by the app's stall-based reconnect in stream/domain.rs, which avoids the
    // version-specific quirks of ffmpeg's -rw_timeout / -timeout on RTSP.)
    let mut args: Vec<&str> = Vec::with_capacity(16);
    match p.transport {
        Transport::Tcp => args.extend(["-rtsp_transport", "tcp"]),
        Transport::Udp => args.extend(["-rtsp_transport", "udp"]),
        Transport::Auto => {}
    }
    args.extend([
        "-i", &url,
        "-an",
        "-vf", &scale,
        "-pix_fmt", "gray",
        "-f", "rawvideo",
        "-r", &fps,
        "-loglevel", "error",
        "pipe:1",
    ]);

    let mut child = Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped()) // captured so we can tell auth from network
        .spawn()
        .map_err(|e| format!("ffmpeg not available: {e}"))?;

    let mut stdout = child.stdout.take().ok_or("no ffmpeg stdout")?;
    let stderr = child.stderr.take().ok_or("no ffmpeg stderr")?;
    let (frames_tx, frames_rx) = std::sync::mpsc::channel::<GrayImage>();
    let (ended_tx, ended_rx) = std::sync::mpsc::channel::<StreamEnd>();
    let frame_bytes = p.size.bytes();
    let (w, h) = (p.size.w, p.size.h);

    // Last stderr line, filled by its own thread so ffmpeg never blocks writing
    // stderr while we're reading the binary frame stream on stdout.
    let last_err: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let last_err_w = last_err.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(mut g) = last_err_w.lock() {
                *g = line;
            }
        }
    });

    std::thread::spawn(move || {
        let mut buf = vec![0u8; frame_bytes];
        loop {
            // A full raw gray frame is exactly w*h bytes; EOF/err means ffmpeg
            // died or the pipe was closed by `child.kill()`.
            match stdout.read_exact(&mut buf) {
                Ok(()) => {
                    if let Some(img) = GrayImage::from_raw(w, h, buf.clone()) {
                        if frames_tx.send(img).is_err() {
                            break; // receiver (LiveStream) dropped
                        }
                    }
                }
                Err(_) => {
                    // Let the stderr thread flush ffmpeg's final line, then
                    // classify the stop (auth → fatal, else transient).
                    std::thread::sleep(Duration::from_millis(120));
                    let reason = last_err.lock().map(|g| g.clone()).unwrap_or_default();
                    let _ = ended_tx.send(StreamEnd::classify(&reason));
                    break;
                }
            }
        }
    });

    Ok(StreamChannels { child, frames_rx, ended_rx })
}

/// Start playing a feed described by `params`. The returned `LiveStream` kills
/// ffmpeg on drop and supervises its own reconnects. `connecting` lights the
/// top-bar indicator until the first frame arrives.
pub fn start(
    params: SpawnParams,
    device_ip: String,
    connecting: ActivityGuard,
) -> Result<LiveStream, String> {
    let ch = spawn_ffmpeg(&params)?;
    Ok(LiveStream::new(ch, params, device_ip, connecting))
}
