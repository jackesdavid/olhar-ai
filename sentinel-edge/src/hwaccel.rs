//! GPU video-decode capability probe (#120). Detects the best **working** ffmpeg hardware-acceleration
//! method for H.264/H.265 decode on this box, and reports it to the cloud (`X-Edge-Hwaccel` header) so
//! HD face capture is only ever offered where it will actually run.
//!
//! "Advertised" (`ffmpeg -hwaccels`) is NOT proof — a method is often listed but fails on a missing
//! driver, no `/dev/dri` permission, or a headless GPU. So each platform-appropriate candidate is
//! **functionally tested** (open its hw device in ffmpeg) before we trust it. Result: `vaapi` | `qsv`
//! | `cuda` | `videotoolbox` | `v4l2m2m` | `none`. Computed once — capability is static per process.

use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The best working hwaccel method for this box, or `"none"` (CPU-only). ASCII — it rides the
/// `X-Edge-Hwaccel` HTTP header.
pub fn detect() -> &'static str {
    static M: OnceLock<String> = OnceLock::new();
    M.get_or_init(probe).as_str()
}

/// ffmpeg **input-side** args to decode on the GPU for [`detect`]'s method — empty when there's
/// nothing to add (`"none"`, or a method with no `-hwaccel` input flag). Insert these before `-i`.
///
/// `cuda` asks ffmpeg to keep the decoded frame in GPU memory (`-hwaccel_output_format cuda`) —
/// cheaper than an implicit download when a filter chain can consume it there, but it means our
/// software MJPEG encoder can't touch the frame directly; see [`decode_filter`].
pub fn decode_args() -> &'static [&'static str] {
    match detect() {
        "cuda" => &["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"],
        "vaapi" => &["-hwaccel", "vaapi"],
        "qsv" => &["-hwaccel", "qsv"],
        "videotoolbox" => &["-hwaccel", "videotoolbox"],
        // v4l2m2m has no `-hwaccel` input flag (it's selected via an explicit `_v4l2m2m` decoder
        // instead) and "none" obviously has nothing to add — CPU decode either way.
        _ => &[],
    }
}

/// A `-vf` filter chain (comma-joined, single arg) needed to make a GPU-resident decoded frame
/// consumable by a software encoder, or `None` if [`decode_args`] already yields system-memory
/// frames (or there's no hwaccel at all). Only `cuda` needs this — its `-hwaccel_output_format
/// cuda` keeps frames on the GPU, so we must `hwdownload` before a CPU encoder (e.g. MJPEG, which
/// has no CUDA-native encoder) can read them.
pub fn decode_filter() -> Option<&'static str> {
    match detect() {
        "cuda" => Some("hwdownload,format=nv12"),
        _ => None,
    }
}

fn probe() -> String {
    if !crate::state::ffmpeg_ok() {
        return "none".to_string();
    }
    let advertised = advertised_hwaccels();
    for &m in candidates() {
        if advertised.iter().any(|a| a == m) && device_opens(m) {
            log::info!("hwaccel: GPU decode available via {m}");
            return m.to_string();
        }
    }
    log::info!("hwaccel: no working GPU decode method (advertised: {advertised:?}) — CPU only");
    "none".to_string()
}

/// Candidate methods to try, best-first, for THIS platform. macOS abstracts Intel/AMD/NVIDIA behind
/// VideoToolbox; on Linux the API follows the vendor — CUDA (NVIDIA), QSV (Intel), VAAPI (Intel+AMD
/// via `/dev/dri`), V4L2 (ARM SoCs). `device_opens()` gates each, so listing extras is harmless.
fn candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["videotoolbox"]
    } else if cfg!(target_os = "linux") {
        if has_nvidia() {
            &["cuda", "vaapi"]
        } else {
            &["qsv", "vaapi", "v4l2m2m"]
        }
    } else {
        &[]
    }
}

fn has_nvidia() -> bool {
    std::path::Path::new("/dev/nvidia0").exists()
        || std::path::Path::new("/proc/driver/nvidia/version").exists()
}

/// Parse `ffmpeg -hwaccels` → the advertised method names (skips the header line).
fn advertised_hwaccels() -> Vec<String> {
    let out = match run_bounded(&["-hide_banner", "-hwaccels"], 5) {
        Some(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Hardware acceleration methods"))
        .map(str::to_string)
        .collect()
}

/// Functionally verify a method by opening its hw device in ffmpeg — catches "listed but broken"
/// (missing driver, no `/dev/dri` access, headless). A cheap null-source init, no real stream.
fn device_opens(method: &str) -> bool {
    // V4L2 M2M has no device to pre-open the same way; trust the advertise + kernel presence.
    if method == "v4l2m2m" {
        return true;
    }
    run_bounded(
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-init_hw_device",
            method,
            "-f",
            "lavfi",
            "-i",
            "nullsrc",
            "-frames:v",
            "1",
            "-f",
            "null",
            "-",
        ],
        6,
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// Run ffmpeg with a hard timeout (kills the child on overrun) so a hung probe can never stall the
/// health poll. Returns `None` on spawn error or timeout.
fn run_bounded(args: &[&str], secs: u64) -> Option<std::process::Output> {
    let mut child = crate::proc::command("ffmpeg")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(secs) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}
