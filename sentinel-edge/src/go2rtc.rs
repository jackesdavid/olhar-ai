//! go2rtc (a.k.a. `sentinel-rtc`) lifecycle, when the edge owns the sidecar
//! (`EDGE_OWN_GO2RTC=1`). Otherwise the edge just uses whatever go2rtc is already on
//! :1984 (e.g. spawned by the desktop app) plus the ONVIF/ffmpeg fallback — so the
//! edge works either way without fighting over the port.
use app_core::go2rtc;

use crate::state::AppState;

/// (Re)start the bundled go2rtc for the current device set. Idempotent: only
/// (re)writes the config + restarts when the desired streams changed or the prior
/// child exited (mirrors the desktop shell). No-op if the binary is missing.
pub fn respawn(state: &AppState) {
    let yaml = {
        let devices = state.devices.lock().unwrap();
        go2rtc::config_yaml(&devices)
    };
    let mut guard = state.go2rtc.lock().unwrap();

    let alive = guard.as_mut().map(|c| matches!(c.try_wait(), Ok(None))).unwrap_or(false);
    let unchanged = std::fs::read_to_string(&state.go2rtc_config).map(|p| p == yaml).unwrap_or(false);
    if alive && unchanged {
        return; // already serving the right streams
    }

    if let Err(e) = std::fs::write(&state.go2rtc_config, &yaml) {
        log::error!("write go2rtc config: {e}");
        return;
    }
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if !state.go2rtc_bin.exists() {
        log::warn!(
            "go2rtc binary not found at {:?} — set GO2RTC_BIN; using ONVIF fallback for snapshots",
            state.go2rtc_bin
        );
        return;
    }
    match std::process::Command::new(&state.go2rtc_bin)
        .arg("-config")
        .arg(&state.go2rtc_config)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            log::info!("go2rtc started (pid {}) → {}", child.id(), go2rtc::BASE);
            // Tee the sidecar's stdout/stderr into the shared log ring (target "go2rtc")
            // so stream connect/disconnect + codec issues show up in the cloud log view.
            if let Some(out) = child.stdout.take() {
                pipe_into_ring(out);
            }
            if let Some(err) = child.stderr.take() {
                pipe_into_ring(err);
            }
            *guard = Some(child);
        }
        Err(e) => log::error!("start go2rtc: {e}"),
    }
}

/// Spawn a reader thread that forwards each line of a go2rtc pipe into the log ring.
fn pipe_into_ring<R: std::io::Read + Send + 'static>(reader: R) {
    use std::io::BufRead;
    let buf = crate::logbuf::LogBuf::global();
    std::thread::spawn(move || {
        let mut lines = std::io::BufReader::new(reader).lines();
        while let Some(Ok(line)) = lines.next() {
            if !line.trim().is_empty() {
                buf.push("info", "go2rtc", line);
            }
        }
    });
}
