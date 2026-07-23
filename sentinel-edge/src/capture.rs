//! Persistent per-camera frame decoder (#120). One **supervised**, long-lived decoder process reads
//! go2rtc's WARM RTSP re-serve (`rtsp://127.0.0.1:8554/cam_<ip>[_hd]`) → a continuous MJPEG stream, so
//! the frame hub reads the freshest decoded frame with **no per-request `frame.jpeg` cold-start**
//! (which measured ~490 ms/grab). The consumer keeps go2rtc's producer warm on its own.
//!
//! **Two interchangeable [`Backend`]s** (card #138, `docs/plans/edge-gstreamer-backend.md`): `ffmpeg`
//! (default) or `gstreamer`, picked per box via `RecordingConfig.media_backend`. Both emit the SAME
//! wire format — concatenated JPEG images on stdout — so [`read_mjpeg`] doesn't care which produced
//! them. This is a coexistence/measurement phase, not a permanent fork; see the plan doc.
//!
//! **Resilient by design:** the decoder exits (stream drop) OR wedges (alive but no frames) are both
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

use crate::media::Backend;

/// Ensure a supervised decoder is running for `ip` on the sub/HD stream. Idempotent — spawns the
/// supervisor exactly once per camera (the `hd` of the first call wins for its lifetime; the setting
/// is boot-time anyway). `media_backend` is the box's `RecordingConfig.media_backend` ("ffmpeg" or
/// "gstreamer", card #138) — resolved once here, at the same point the caller already reads
/// `face_capture_stream` (`frames.rs`).
pub fn ensure(ip: &str, hd: bool, media_backend: &str) {
    let cam = cam_for(ip);
    if cam.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let backend = Backend::resolve(media_backend);
    let ip = ip.to_string();
    std::thread::spawn(move || supervise(&ip, hd, cam, backend));
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

/// Build the full arg list for the decode ffmpeg, optionally with GPU decode (`use_hwaccel`).
/// `-hwaccel …` must precede `-i`; the download filter (if any) goes on the encode side, right
/// before the output format.
fn ffmpeg_args(ip: &str, hd: bool, use_hwaccel: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    if use_hwaccel {
        args.extend(crate::hwaccel::decode_args().iter().map(|s| s.to_string()));
    }
    args.extend(["-rtsp_transport".into(), "tcp".into(), "-i".into(), stream_url(ip, hd)]);
    args.extend(["-an".into(), "-r".into(), "6".into(), "-q:v".into(), "2".into()]);
    if use_hwaccel {
        if let Some(filter) = crate::hwaccel::decode_filter() {
            args.extend(["-vf".into(), filter.to_string()]);
        }
    }
    args.extend(["-f".into(), "mjpeg".into(), "pipe:1".into()]);
    args
}

fn spawn_ffmpeg(ip: &str, hd: bool, use_hwaccel: bool) -> std::io::Result<Child> {
    // Decode the warm RTSP → MJPEG at a modest fps (the hub throttles to ~4fps; 6 keeps frames fresh
    // without over-encoding). High JPEG quality (-q:v 2) so the derived face crops stay sharp.
    // `use_hwaccel` asks for GPU decode via `crate::hwaccel` when the box detected one (#120 probe);
    // `supervise()` turns it off for a camera whose GPU decode fails in practice even though the
    // synthetic probe passed (e.g. a driver that only breaks under real load).
    crate::proc::command("ffmpeg")
        .args(ffmpeg_args(ip, hd, use_hwaccel))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
}

/// Same job as [`spawn_ffmpeg`] (warm RTSP → concatenated-JPEG on stdout at ~6fps), via GStreamer
/// (card #138). `decodebin` auto-negotiates depay/parse/decoder for whatever codec the camera's
/// stream actually carries (H264 or H265) — codec-agnostic, mirroring ffmpeg's own auto-detection —
/// so this one pipeline covers every camera without a per-codec branch. `videoconvert` normalizes
/// whatever raw format `decodebin` lands on (I420/NV12/…) to what `jpegenc` expects.
///
/// **Software decode only for now** (Phase 2a of the plan) — no HW-decode element
/// (`nvv4l2decoder`/`vaapih264dec`/`vtdec`) is selected yet, so the first ffmpeg-vs-gstreamer
/// measurement is apples-to-apples (both CPU-only). Mapping the box's detected [`crate::hwaccel`]
/// method to a GStreamer HW element is Phase 3, once SW parity is measured.
///
/// `GST_PLUGIN_FEATURE_RANK` zero-ranks the known HW-decoder elements so `decodebin` can't
/// autoplug one even if it's installed and otherwise the highest-ranked H264/H265 decoder on the
/// box — found live on the Jetson test box: `videoconvert` only accepts system-memory
/// `video/x-raw`, but once `gstreamer1.0-plugins-bad` is present too (installed by `install.sh`
/// for a future Phase 3), `decodebin` prefers `nvv4l2decoder`, whose output is Tegra `NVMM`
/// memory — `videoconvert` can't take that, so the pipeline fails outright with
/// `not-negotiated`, for EVERY camera. Without this pin, capture silently breaks the moment the
/// box happens to have any GPU-decoder GStreamer plugin installed.
fn spawn_gst(ip: &str, hd: bool) -> std::io::Result<Child> {
    let pipeline = format!(
        "rtspsrc location={} protocols=tcp latency=200 ! decodebin ! videoconvert ! videorate ! \
         video/x-raw,framerate=6/1 ! jpegenc quality=85 ! fdsink fd=1 sync=false",
        stream_url(ip, hd)
    );
    crate::proc::command("gst-launch-1.0")
        .arg("-q") // quiet: no progress/property spam on stdout — only the fdsink's JPEG bytes
        .args(pipeline.split_whitespace())
        .env(
            "GST_PLUGIN_FEATURE_RANK",
            "nvv4l2decoder:0,nvv4l2h264dec:0,nvv4l2h265dec:0,vaapih264dec:0,vaapih265dec:0,\
             v4l2h264dec:0,v4l2h265dec:0,msdkh264dec:0,msdkh265dec:0,d3d11h264dec:0,d3d11h265dec:0,\
             vtdec_hw:0",
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
}

fn spawn(ip: &str, hd: bool, backend: Backend, use_hwaccel: bool) -> std::io::Result<Child> {
    match backend {
        Backend::Ffmpeg => spawn_ffmpeg(ip, hd, use_hwaccel),
        Backend::Gstreamer => spawn_gst(ip, hd),
    }
}

/// Consecutive fast (<30s) failures while GPU decode is on before we give up on it for this
/// camera and fall back to CPU. The synthetic probe in `hwaccel::detect()` (open the hw device
/// once, decode one null frame) can pass while the real thing still fails under load — a driver
/// that only breaks on a genuine RTSP stream, or a codec profile it doesn't actually support.
const HWACCEL_FAIL_LIMIT: u32 = 2;

fn supervise(ip: &str, hd: bool, cam: Arc<Cam>, backend: Backend) {
    log::info!(
        "capture-decoder[{ip}]: starting (stream={}, backend={})",
        if hd { "HD (main)" } else { "preview (sub)" },
        backend.label()
    );
    let mut backoff = Duration::from_millis(500);
    let mut fails: u32 = 0;
    // GPU-decode fallback bookkeeping only applies to the ffmpeg branch — the gstreamer branch
    // (Phase 2a) is software-only by design, so there's nothing to fall back from there yet.
    let mut use_hwaccel = backend == Backend::Ffmpeg && crate::hwaccel::detect() != "none";
    let mut hw_fails: u32 = 0;
    loop {
        let ran_from = Instant::now();
        match spawn(ip, hd, backend, use_hwaccel) {
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
            Err(e) => log::warn!("capture-decoder[{ip}]: {} spawn failed: {e}", backend.label()),
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
            hw_fails = 0; // a healthy run vindicates GPU decode too — don't carry stale fails
        } else {
            fails += 1;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            if fails == 5 {
                log::warn!(
                    "capture-decoder[{ip}]: repeated {} failures — backing off to 30s (frame.jpeg fallback active)",
                    backend.label()
                );
            }
            // GPU decode fast-failing in practice despite passing the boot-time probe (#120) —
            // fall back to CPU for this camera rather than retrying the same broken command forever.
            if use_hwaccel {
                hw_fails += 1;
                if hw_fails >= HWACCEL_FAIL_LIMIT {
                    use_hwaccel = false;
                    log::warn!(
                        "capture-decoder[{ip}]: GPU decode ({}) failing in practice — falling back to CPU decode for this camera",
                        crate::hwaccel::detect()
                    );
                }
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
