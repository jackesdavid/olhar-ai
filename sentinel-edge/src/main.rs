//! `sentinel-edge` — the edge gateway.
//!
//! Phase 2 of the cloud decomposition: the **only** thing that touches cameras. It
//! wraps go2rtc (`sentinel-rtc`) + ONVIF and serves on-demand JPEG snapshots over
//! REST, so out-of-process services reach the LAN-bound cameras without speaking
//! RTSP/ONVIF themselves. `sentinel-flow`'s `FrameSource` becomes a client of this
//! (`EDGE_URL`), fetching binary frames instead of base64'ing them onto SSE.
//!
//! Cameras are LAN-bound (ONVIF 192.168.x.x; go2rtc binds 1984/8554/8555), so an
//! edge tier is mandatory however cloud the rest becomes.
//!
//! Run:
//!   DATABASE_URL=postgres://sentinel:sentinel@localhost/sentinel \
//!   EDGE_BIND=0.0.0.0:8788 cargo run -p sentinel-edge   # or: task edge
//!
//! Optional env: `EDGE_OWN_GO2RTC=1` to spawn/own the go2rtc sidecar (set
//! `GO2RTC_BIN` + `GO2RTC_CONFIG`); otherwise it uses whatever go2rtc is on :1984.
mod capture;
mod clip_upload;
mod enroll;
mod envfile;
mod face;
mod frames;
mod go2rtc;
#[cfg(feature = "gst-linked")]
mod gst_mse;
#[cfg(feature = "gst-linked")]
mod gst_mux;
mod http;
mod hwaccel;
mod logbuf;
mod media;
mod metrics;
mod mog2;
mod motion;
mod paths;
mod proc;
mod recording;
mod state;
mod tailscale;
mod tracking;

use state::AppState;

/// Check control for the latest published edge version. If it differs from our own,
/// spawn install.sh --update in the background — the script stops the service, swaps
/// the binary, and restarts it, so the service manager relaunches us on the new build.
/// This means the UI Restart button doubles as an update trigger: restart → startup →
/// version check → auto-update. No more bootstrapping problem between releases.
fn maybe_self_update(control_url: &str) {
    let current = env!("CARGO_PKG_VERSION");
    let url = format!("{control_url}/edge-latest");
    let resp = match ureq::get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            log::debug!("version check skipped: {e}");
            return;
        }
    };
    let latest = resp
        .into_json::<serde_json::Value>()
        .ok()
        .and_then(|v| {
            v.get("version")
                .and_then(|s| s.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    if latest.is_empty() || latest == current {
        log::debug!("edge {current} is up to date (latest={latest})");
        return;
    }
    // The Unix path: shell out to install.sh, which stops the service, swaps the binary, and
    // restarts. There's no `sh` or `install.sh` on Windows — and Windows can't overwrite a running
    // .exe anyway, so its self-update is a different mechanism (a PowerShell installer that does the
    // rename dance; card #83). Until that lands, a Windows box updates by re-running the installer,
    // so we just note the available version instead of trying to run a shell that isn't there.
    #[cfg(unix)]
    {
        log::warn!("auto-update: {current} → {latest} — spawning install.sh --update");
        let script = format!(
            "curl -fsSL {control_url}/install.sh | sh -s -- --update --control {control_url}"
        );
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .spawn();
        // install.sh stops + replaces the binary + restarts the service on its own.
    }
    #[cfg(not(unix))]
    {
        log::warn!("auto-update: {current} → {latest} available — re-run the installer to update (self-update is installer-managed on this platform)");
    }
}

fn main() {
    // `sentinel-edge --version` — print the build version and exit (install.sh runs this to
    // echo what it just installed; also handy for `systemctl`-managed boxes).
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("sentinel-edge {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // `sentinel-edge gst-selftest` — build the real ingest pipeline against a synthetic source and
    // exit 0/1. The macOS installer bundles its own GStreamer runtime (card #138); this is how the
    // postinstall proves that runtime is complete on THIS machine before trusting the box to it,
    // instead of discovering a missing plugin the first time a camera connects.
    #[cfg(feature = "gst-linked")]
    if std::env::args().any(|a| a == "gst-selftest") {
        std::process::exit(match media::gst_selftest() {
            Ok(()) => {
                println!("gst-selftest: OK");
                0
            }
            Err(e) => {
                eprintln!("gst-selftest: FAILED — {e}");
                1
            }
        });
    }

    // Capture logs into a shared ring (also teed to stderr) so the box can push them up
    // for the in-UI log viewer — the edge is no longer a black box.
    let logs = logbuf::init();

    // Read our own edge.env. systemd re-reads it on every start, but launchd bakes the values into
    // the plist at INSTALL time — so a box that acquires its identity at runtime (enrollment, #81)
    // would never see it on macOS. Loading it ourselves makes every platform behave the same.
    envfile::load();

    let bind = std::env::var("EDGE_BIND").unwrap_or_else(|_| "0.0.0.0:8788".to_string());

    // Stamp the build version + platform in the very first log line — it rides the cloud log
    // viewer, so an operator sees exactly which build a box is running (matches the Boxes badge).
    log::info!(
        "sentinel-edge v{} starting on {}",
        env!("CARGO_PKG_VERSION"),
        state::platform()
    );

    // No identity yet → serve nothing but the enrollment page and wait for a pairing code (#81).
    // Everything below this point assumes a box token, and one of those assumptions is dangerous:
    // a camera pull with no token falls back to the DEFAULT TENANT, so an unenrolled box that took
    // the normal path would pull down somebody else's cameras. It stops here instead.
    if !enroll::enrolled() {
        let control_url = AppState::control_url();
        log::warn!(
            "this box has no identity yet — open http://{bind} on this machine and enter the \
             pairing code from your Boxes page (control: {control_url})"
        );
        let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
        rt.block_on(async move {
            let listener = match tokio::net::TcpListener::bind(&bind).await {
                Ok(l) => l,
                Err(e) => {
                    log::error!("failed to bind {bind}: {e}");
                    std::process::exit(1);
                }
            };
            log::info!("waiting to be paired on http://{bind}");
            if let Err(e) = axum::serve(listener, enroll::router(control_url))
                .with_graceful_shutdown(shutdown_signal())
                .await
            {
                log::error!("server error: {e}");
                std::process::exit(1);
            }
        });
        return;
    }

    // Auto-update: if control reports a newer version, run install.sh --update and let
    // the service manager relaunch us with the new binary. This means any restart
    // (UI button, crash, deploy) automatically picks up the latest edge release.
    maybe_self_update(&AppState::control_url());

    // Join the tailnet + expose go2rtc over it (opt-in via TS_AUTHKEY) FIRST, so the
    // detected EDGE_PUBLIC_HOST is reported to control on the very first camera pull.
    tailscale::ensure_up();

    // Config-light boot: pull the camera list from the control plane (cache fallback).
    // No database — the appliance carries no Postgres.
    let state = AppState::boot();

    // ffmpeg matters only while go2rtc does: it is go2rtc that shells out to it for JPEG frames.
    // On a box where GStreamer owns the media path there is no go2rtc and no ffmpeg, so warning
    // about a missing binary nobody uses would be noise on the health badge and the log viewer.
    if !state.own_go2rtc {
        log::info!("media path is GStreamer — ffmpeg is not required on this box");
    } else if state::ffmpeg_ok() {
        log::info!("ffmpeg found on PATH");
    } else {
        log::warn!(
            "ffmpeg NOT found on PATH — go2rtc JPEG frames (snapshots / GetCurrentFrame) will fail. \
             Install it (Linux: apt install ffmpeg · macOS: brew install ffmpeg)."
        );
    }

    // 443 is what the deployed gateway dials for live video. Publish the port BEFORE re-serving, so
    // the periodic `ensure_up` keeps honouring it instead of resetting it to go2rtc behind our back.
    if !state.own_go2rtc {
        tailscale::set_live_port(8788);
        tailscale::serve_live(8788);
    }

    if state.own_go2rtc {
        go2rtc::respawn(&state);
        log::info!("edge owns go2rtc (EDGE_OWN_GO2RTC)");
    } else {
        log::info!("edge using external go2rtc on :1984 (+ ONVIF fallback)");
    }
    // Report the initially-resolved stream set so the Devices dot is correct from boot.
    state.push_streaming();
    // If cameras_always_warm is enabled, pre-open each camera's go2rtc RTSP session so
    // recordings start instantly without a cold-start delay.
    state.warm_cameras();

    // The frame hub: one source thread per camera that fetches + decodes the frame ONCE, runs the
    // continuous MOG2 motion pass (POSTing normalized motion events to control), and
    // publishes the decoded frame to every other consumer — so a camera with four tracking jobs
    // costs one decode, not five.
    frames::spawn(&state);
    // Continuous object tracking (one YOLOX+SORT worker per camera). Enabled per tracking job —
    // i.e. per graph `OnEvent` node wired to a camera — not by any box-wide switch (TRACK_ENABLED=1
    // only synthesizes a dev fallback job); pushes object_entered/object_exited events to control.
    tracking::spawn(&state);
    // Resource-usage sampler (CPU / memory / disk / GPU) — the latest snapshot rides the
    // heartbeat as the X-Edge-Metrics header so the Boxes page can show if the box is healthy.
    metrics::spawn();

    // Discovery loop: sweep the LAN for ONVIF cameras every 5 min — reconciling any camera
    // that changed IP (matched by stable ONVIF id) and pushing the sweep to control so the
    // cloud Devices page can surface them. Moves self-heal here even without a manual rescan.
    {
        let disco = std::sync::Arc::clone(&state);
        std::thread::spawn(move || loop {
            disco.rediscover();
            // Re-assert the tailnet exposure AND correct a drifted reported host, so a box that came
            // up before its node settled heals itself here — no restart, no terminal (the live-video
            // self-heal).
            tailscale::reassert();
            // Start ring buffers for any newly discovered cameras (idempotent per IP).
            disco.warm_cameras();
            std::thread::sleep(std::time::Duration::from_secs(300));
        });
    }

    // Config long-poll: block on control until the web changes a camera (or 25s), then
    // apply immediately — so cloud edits (e.g. setting credentials) take effect in ~1s.
    // After each cycle, drain pending cloud commands: rescan (re-run discovery now) and
    // restart (exit so the service manager relaunches us → re-runs tailscale up + serve).
    {
        let sync = std::sync::Arc::clone(&state);
        std::thread::spawn(move || loop {
            sync.reload(25);
            let cmd = sync.check_commands();
            if cmd.rescan {
                log::info!("rescan requested from the cloud — running discovery now");
                sync.rediscover();
            }
            if cmd.restart {
                log::warn!("restart requested from the cloud — exiting for the service manager to relaunch");
                std::process::exit(0);
            }
            if cmd.update {
                // Exit so the service manager relaunches us. `maybe_self_update()` runs at
                // startup and handles the actual binary swap — no need to spawn install.sh
                // from inside the running process (launchd's SIGKILL would kill it anyway).
                log::warn!("update requested from the cloud — exiting for service manager to relaunch + auto-update");
                std::process::exit(0);
            }
        });
    }

    // Log push loop: ship new log lines (edge + go2rtc) up to control every few seconds so
    // the cloud log viewer shows a near-live tail. Best-effort; stdout remains the source.
    {
        let logs_state = std::sync::Arc::clone(&state);
        std::thread::spawn(move || {
            let mut cursor = 0u64;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let (lines, next) = logs.since(cursor);
                if !lines.is_empty() && logs_state.push_logs(&lines) {
                    cursor = next;
                }
            }
        });
    }

    // Storage eviction + disk-alert loop: every 60 s, evict old clips when over the configured
    // limit, and push an `edge:full-disk` event to control when usage exceeds 90%.
    {
        let evict_state = std::sync::Arc::clone(&state);
        std::thread::spawn(move || {
            // Minimum interval between disk-full alerts (avoid spamming control).
            let alert_interval = std::time::Duration::from_secs(10 * 60);
            let mut last_alert: Option<std::time::Instant> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let mgr = evict_state.recording.clone();
                let evicted = mgr.evict_if_over_limit();
                if evicted > 0 {
                    log::info!("storage: evicted {evicted} clip(s) to stay under limit");
                }
                let stats = mgr.storage_stats();
                // Fire alert when >90% full — rate-limited to once per alert_interval.
                if stats.pct >= 90 {
                    let should_alert = last_alert
                        .map(|t| t.elapsed() >= alert_interval)
                        .unwrap_or(true);
                    if should_alert {
                        log::warn!(
                            "storage: {}% full ({}/{} bytes) — pushing edge:full-disk alert",
                            stats.pct,
                            stats.used_bytes,
                            stats.max_bytes
                        );
                        evict_state.push_disk_alert(
                            stats.used_bytes,
                            stats.max_bytes,
                            stats.pct,
                            &stats.path,
                        );
                        last_alert = Some(std::time::Instant::now());
                    }
                }
            }
        });
    }

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    rt.block_on(async move {
        let app = http::router(state);
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("failed to bind {bind}: {e}");
                std::process::exit(1);
            }
        };
        log::info!("sentinel-edge listening on http://{bind} (GET /snapshot/<ip>)");
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            log::error!("server error: {e}");
            std::process::exit(1);
        }
    });
    log::info!("sentinel-edge shut down");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    log::info!("shutdown signal received");
}
