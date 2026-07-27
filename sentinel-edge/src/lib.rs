//! `sentinel-edge` — the edge gateway. The **only** thing that touches cameras.
//!
//! It ingests each camera's RTSP over the LAN and decodes it **in-process with linked GStreamer**
//! (card #138) — no go2rtc sidecar, no ffmpeg, no tailnet. Live video is pushed up the edge's own
//! OUTBOUND connection to the gateway (card #144), and cloud→edge commands ride an outbound SSE
//! stream, so the edge needs no inbound reachability. It reaches control outbound over HTTPS and its
//! cameras over the LAN directly.
//!
//! Run:
//!   EDGE_BIND=0.0.0.0:8788 cargo run -p sentinel-edge --features gst-linked   # or: task edge
mod capture;
mod clip_upload;
mod command_stream;
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
#[cfg(feature = "gst-linked")]
mod live_push;
mod logbuf;
mod media;
mod metrics;
mod mog2;
mod motion;
mod paths;
mod proc;
mod recording;
mod state;
mod tracking;

use state::AppState;

/// Check control for the latest published edge version. If it differs from our own,
/// spawn install.sh --update in the background — the script stops the service, swaps
/// the binary, and restarts it, so the service manager relaunches us on the new build.
/// This means the UI Restart button doubles as an update trigger: restart → startup →
/// version check → auto-update. No more bootstrapping problem between releases.
#[cfg(not(target_os = "ios"))] // the embedded iOS edge is updated with the app, not self-updated
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

pub fn run() {
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

    // An appliance binds all interfaces (the gateway/control reach it over the LAN/tailnet); an
    // embedded edge on a phone only ever serves the app on the same device, and iOS is happiest with
    // an explicit localhost bind — so default per platform (card #144). `EDGE_BIND` still overrides.
    let bind = std::env::var("EDGE_BIND").unwrap_or_else(|_| default_bind());

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
    // Appliance-only: iOS can't run install.sh, and the phone updates via the App Store, not this.
    #[cfg(not(target_os = "ios"))]
    maybe_self_update(&AppState::control_url());

    // Config-light boot: pull the camera list from the control plane (cache fallback).
    // No database — the appliance carries no Postgres.
    let state = AppState::boot();

    // Media path: the in-process linked GStreamer decoder + outbound live dial (cards #138 + #144).
    // No tailnet, no go2rtc sidecar, no ffmpeg — the edge reaches control outbound over HTTPS and its
    // cameras over the LAN directly. The `tailscale`/`go2rtc`/`ffmpeg` binaries are retired here.
    // The legacy EDGE_OWN_GO2RTC opt-in is never set on a GStreamer box; kept only for a bare-binary
    // dev box that hasn't linked GStreamer yet.
    if state.own_go2rtc {
        go2rtc::respawn(&state);
        log::info!("edge owns go2rtc (legacy EDGE_OWN_GO2RTC path)");
    } else {
        log::info!("media: in-process GStreamer — no go2rtc, no ffmpeg, no tailnet");
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
            // Start ring buffers for any newly discovered cameras (idempotent per IP).
            disco.warm_cameras();
            std::thread::sleep(std::time::Duration::from_secs(300));
        });
    }

    // Config long-poll: block on control until the web changes a camera (or 25s), then
    // apply immediately — so cloud edits (e.g. setting credentials) take effect in ~1s.
    // After each cycle, drain pending cloud commands: rescan (re-run discovery now) and
    // restart (exit so the service manager relaunches us on a fresh process).
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

    // Cloud→edge command stream (card #144): hold one outbound SSE to control so the cloud can
    // command this box (record, …) WITHOUT reaching it inbound — the tailnet replacement. Commands
    // dispatch to the edge's own local HTTP API, so the cloud path reuses the exact local flow.
    {
        let control_url = state.control_url.clone();
        let port = bind.rsplit(':').next().unwrap_or("8788").to_string();
        let local_api = format!("http://127.0.0.1:{port}");
        let cmd_state = std::sync::Arc::clone(&state);
        std::thread::spawn(move || command_stream::run(control_url, local_api, cmd_state));
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

/// The default listen address — localhost on iOS (embedded, app-only), all interfaces elsewhere.
fn default_bind() -> String {
    #[cfg(target_os = "ios")]
    {
        "127.0.0.1:8788".to_string()
    }
    #[cfg(not(target_os = "ios"))]
    {
        "0.0.0.0:8788".to_string()
    }
}

// ---- C ABI entry points (card #144) — the phone's native module starts/stops the edge here.
// On iOS/Android the app is the process; there is no `fn main`. `start_edge` boots the same `run()`
// on a background thread and returns; `stop_edge` requests shutdown. This is what makes the edge a
// library the app links, not a standalone binary.
use std::sync::atomic::{AtomicBool, Ordering};

static EDGE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Boot the edge on a background thread with the phone's identity. `box_token` (from self-register,
/// #143) makes the edge **enrolled** so `run()` takes the full path — pull cameras, record, report
/// health — instead of the pairing stub; `control_url` is the cloud it talks to (the embedded edge
/// has no env). Both may be empty/null to boot unenrolled (the earlier demo mode). Idempotent.
///
/// # Safety
/// `box_token` and `control_url` must be null or valid NUL-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn start_edge(
    box_token: *const std::os::raw::c_char,
    control_url: *const std::os::raw::c_char,
) {
    if EDGE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let cstr = |p: *const std::os::raw::c_char| -> Option<String> {
        if p.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(p).to_str().ok().map(str::to_string).filter(|s| !s.trim().is_empty())
    };
    if let Some(t) = cstr(box_token) {
        std::env::set_var("BOX_TOKEN", t.trim());
    }
    if let Some(u) = cstr(control_url) {
        std::env::set_var("CONTROL_URL", u.trim_end_matches('/'));
    }
    // The phone always runs the linked-GStreamer media path (no go2rtc/ffmpeg to spawn on iOS).
    std::env::set_var("EDGE_MEDIA_BACKEND", "gstreamer-linked");

    std::thread::spawn(|| {
        run();
        EDGE_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Request edge shutdown. The full teardown wiring lands with the native module (Phase 2); for now
/// this flips the flag so `start_edge` can be called again after the process is torn down.
#[no_mangle]
pub extern "C" fn stop_edge() {
    EDGE_RUNNING.store(false, Ordering::SeqCst);
}
