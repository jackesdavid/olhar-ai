//! Persistent per-camera frame decoder (#120). One **supervised**, long-lived `ffmpeg` reads go2rtc's
//! WARM RTSP re-serve (`rtsp://127.0.0.1:8554/cam_<ip>[_hd]`) → a continuous MJPEG stream, so the frame
//! hub reads the freshest decoded frame with **no per-request `frame.jpeg` cold-start** (which measured
//! ~490 ms/grab). The ffmpeg consumer keeps go2rtc's producer warm on its own.
//!
//! **Resilient by design:** ffmpeg exits (stream drop) OR wedges (alive but no frames) are both
//! detected — a watchdog kills a stalled process so the read unblocks, and the supervisor respawns
//! with exponential backoff. The hub always falls back to go2rtc `frame.jpeg` when there's no FRESH
//! frame ([`latest_fresh`] returns `None`), so capture never stops even while a decoder is restarting.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Cam {
    latest: Mutex<Option<(Arc<Vec<u8>>, Instant)>>, // freshest JPEG + when it arrived
    started: AtomicBool,                            // supervisor spawned?
}

fn cams() -> &'static Mutex<HashMap<String, Arc<Cam>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<Cam>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cam_for(ip: &str) -> Arc<Cam> {
    let mut map = cams().lock().unwrap();
    map.entry(ip.to_string())
        .or_insert_with(|| Arc::new(Cam { latest: Mutex::new(None), started: AtomicBool::new(false) }))
        .clone()
}

/// Ensure a supervised decoder is running for `ip` on the sub/HD stream. Idempotent — spawns the
/// supervisor exactly once per camera (the `hd` of the first call wins for its lifetime; the setting
/// is boot-time anyway).
pub fn ensure(ip: &str, hd: bool) {
    let cam = cam_for(ip);
    if cam.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let ip = ip.to_string();
    std::thread::spawn(move || supervise(&ip, hd, cam));
}

/// The freshest decoded JPEG for `ip`, only if newer than `max_age` — else `None` so the hub falls
/// back to go2rtc `frame.jpeg`.
pub fn latest_fresh(ip: &str, max_age: Duration) -> Option<Arc<Vec<u8>>> {
    let cam = cams().lock().unwrap().get(ip).cloned()?;
    let g = cam.latest.lock().unwrap();
    match &*g {
        Some((jpeg, at)) if at.elapsed() <= max_age => Some(Arc::clone(jpeg)),
        _ => None,
    }
}

fn stream_url(ip: &str, hd: bool) -> String {
    let name = if hd {
        app_core::go2rtc::stream_name_hd(ip)
    } else {
        app_core::go2rtc::stream_name(ip)
    };
    // go2rtc's LOCAL RTSP re-serve (creds already injected) — localhost, warm passthrough.
    format!("rtsp://127.0.0.1:8554/{name}")
}

fn spawn_ffmpeg(ip: &str, hd: bool) -> std::io::Result<Child> {
    // Decode the warm RTSP → MJPEG at a modest fps (the hub throttles to ~4fps; 6 keeps frames fresh
    // without over-encoding). High JPEG quality (-q:v 2) so the derived face crops stay sharp.
    crate::proc::command("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-rtsp_transport",
            "tcp",
            "-i",
            &stream_url(ip, hd),
            "-an",
            "-r",
            "6",
            "-q:v",
            "2",
            "-f",
            "mjpeg",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
}

fn supervise(ip: &str, hd: bool, cam: Arc<Cam>) {
    log::info!("capture-decoder[{ip}]: starting (stream={})", if hd { "HD (main)" } else { "preview (sub)" });
    let mut backoff = Duration::from_millis(500);
    let mut fails: u32 = 0;
    loop {
        let ran_from = Instant::now();
        match spawn_ffmpeg(ip, hd) {
            Ok(mut child) => {
                let stdout = match child.stdout.take() {
                    Some(s) => s,
                    None => {
                        let _ = child.kill();
                        let _ = child.wait();
                        std::thread::sleep(backoff);
                        continue;
                    }
                };
                let spawn_at = Instant::now();
                let child = Arc::new(Mutex::new(child));
                // Watchdog: kill a wedged ffmpeg (alive but not producing) so the read below unblocks.
                // Generous warmup (HD streams take a moment); silent (the supervisor logs the policy).
                let wd = {
                    let (cam_w, child_w) = (Arc::clone(&cam), Arc::clone(&child));
                    std::thread::spawn(move || loop {
                        std::thread::sleep(Duration::from_secs(2));
                        let mut c = child_w.lock().unwrap();
                        match c.try_wait() {
                            Ok(Some(_)) => break, // exited on its own → reader will EOF
                            Ok(None) => {
                                let stale = match &*cam_w.latest.lock().unwrap() {
                                    Some((_, at)) => at.elapsed() > Duration::from_secs(8),
                                    None => spawn_at.elapsed() > Duration::from_secs(20),
                                };
                                if stale {
                                    let _ = c.kill();
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    })
                };
                read_mjpeg(stdout, &cam); // blocks until EOF (natural exit or watchdog kill)
                {
                    let mut c = child.lock().unwrap();
                    let _ = c.kill();
                    let _ = c.wait();
                }
                let _ = wd.join();
            }
            Err(e) => log::warn!("capture-decoder[{ip}]: ffmpeg spawn failed: {e}"),
        }
        // Backoff policy (the fix): reset ONLY after a healthy run — a fast-dying ffmpeg must NOT reset
        // the backoff (that was the tight-loop storm). Repeated fast failures cool down HARD so we
        // never hammer go2rtc; the hub keeps working on the frame.jpeg fallback the whole time.
        if ran_from.elapsed() >= Duration::from_secs(30) {
            if fails > 0 {
                log::info!("capture-decoder[{ip}]: recovered — streaming again");
            }
            backoff = Duration::from_millis(500);
            fails = 0;
        } else {
            fails += 1;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            if fails == 5 {
                log::warn!(
                    "capture-decoder[{ip}]: repeated ffmpeg failures — backing off to 30s (frame.jpeg fallback active)"
                );
            }
        }
        std::thread::sleep(backoff);
    }
}

/// Read concatenated MJPEG (SOI `FF D8` … EOI `FF D9`) from ffmpeg's stdout, publishing each complete
/// frame as the camera's latest. Returns on EOF/error (→ supervisor respawns).
fn read_mjpeg(mut stdout: std::process::ChildStdout, cam: &Arc<Cam>) {
    let mut buf: Vec<u8> = Vec::with_capacity(256 * 1024);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        // Extract every complete JPEG (SOI…EOI), keeping the freshest as `latest`.
        loop {
            // Align the buffer to the next SOI, dropping any junk/partial before it.
            match find(&buf, &[0xFF, 0xD8]) {
                Some(0) => {}
                Some(soi) => drop(buf.drain(..soi)),
                None => {
                    buf.clear(); // no frame start in view — reset
                    break;
                }
            }
            if buf.len() < 4 {
                break; // need more bytes before we can find an EOI
            }
            match find(&buf[2..], &[0xFF, 0xD9]) {
                Some(rel) => {
                    let end = 2 + rel + 2; // past the leading SOI + the 2-byte EOI marker
                    let jpeg = buf[..end].to_vec();
                    buf.drain(..end);
                    *cam.latest.lock().unwrap() = Some((Arc::new(jpeg), Instant::now()));
                }
                None => break, // EOI not arrived yet — wait for more data
            }
        }
        if buf.len() > 8 * 1024 * 1024 {
            buf.clear(); // corrupt / no EOI — don't grow unbounded
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
