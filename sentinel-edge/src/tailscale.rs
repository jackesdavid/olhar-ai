//! Tailscale bring-up for the edge box. The edge joins the tailnet and exposes its
//! localhost-only go2rtc (`127.0.0.1:1984`) over the tailnet with a MagicDNS TLS cert
//! — so the central gateway reaches it privately, while go2rtc is never on the LAN or
//! public internet directly.
//!
//! Enrollment is **static** for now: a pre-provisioned auth key + a per-edge hostname
//! and tag (pre-staging the per-tenant identity for later multi-tenancy). The dynamic
//! per-tenant broker (gateway mints short-lived keys) is deferred.
//!
//! All of this is opt-in via `TS_AUTHKEY`. With it unset the edge skips Tailscale
//! entirely (LAN/one-box dev still works); if the `tailscale` CLI/daemon is missing
//! it logs and continues rather than failing the service.
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A `tailscale` CLI command, with the console window suppressed on Windows.
///
/// The binary is resolved via `TAILSCALE_BIN` (default `"tailscale"`, found on PATH). On Windows
/// Tailscale installs to `C:\Program Files\Tailscale\tailscale.exe` and is **not** on PATH by
/// default, so the installer (#83) points `TAILSCALE_BIN` at the full path; on Linux/macOS the bare
/// name resolves.
fn ts() -> Command {
    let bin = std::env::var("TAILSCALE_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "tailscale".to_string());
    let mut cmd = Command::new(bin);
    crate::proc::no_window(&mut cmd);
    cmd
}

/// How long any single `tailscale` invocation may take before we give up on it.
/// Generous enough for a slow control-plane round trip, short enough that a wedged daemon
/// costs us one loop instead of the appliance.
const TS_TIMEOUT: Duration = Duration::from_secs(15);

/// Run a `tailscale` command with a hard timeout, killing it if it overruns.
///
/// **Every** tailscale call must go through here. `Command::output()` waits forever, and the
/// CLI does hang: when the daemon is stopped, `tailscale serve --bg 1984` never returns. Since
/// `ensure_up()` runs on the boot path *before* the HTTP server and the config poll, one hung
/// `serve` didn't merely take the tailnet down — it stopped the edge from booting at all. The
/// box then can't poll, so it can't be told to restart and can't self-update: it stays bricked
/// until someone fixes Tailscale by hand, which is exactly the support call the appliance
/// exists to avoid. A dead tailnet must cost us the tailnet, never the box.
fn run(mut cmd: Command, what: &str) -> Option<std::process::Output> {
    let mut child = match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log::debug!("tailscale {what}: CLI unavailable ({e})");
            return None;
        }
    };

    let deadline = Instant::now() + TS_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    log::warn!(
                        "tailscale {what} hung for {}s — killing it and carrying on (is the daemon stopped?)",
                        TS_TIMEOUT.as_secs()
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                log::debug!("tailscale {what}: wait failed ({e})");
                return None;
            }
        }
    }
    child.wait_with_output().ok()
}

/// If `TS_AUTHKEY` is set, bring the node up and `serve` go2rtc over the tailnet.
/// Idempotent: `tailscale up`/`serve` are safe to re-run. Best-effort — never fatal.
pub fn ensure_up() {
    let Ok(authkey) = std::env::var("TS_AUTHKEY") else {
        log::info!("TS_AUTHKEY unset — skipping Tailscale (LAN/dev mode)");
        return;
    };
    if authkey.trim().is_empty() {
        log::info!("TS_AUTHKEY empty — skipping Tailscale");
        return;
    }
    // Capture whether EDGE_PUBLIC_HOST was an explicit install-time override BEFORE we ever auto-set
    // it below (the OnceLock inside remembers the boot value).
    host_is_explicit();
    let hostname = std::env::var("TS_HOSTNAME").unwrap_or_else(|_| "sentinel-edge".to_string());
    let tags = std::env::var("TS_TAGS")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let mut up = ts();
    // `--reset`: the appliance OWNS its tailscale identity (authkey-driven). Without it, a box
    // that already had tailscale configured (e.g. Bazzite's `ujust` sets `--operator=<user>`)
    // makes `tailscale up` refuse with "changing settings requires mentioning all non-default
    // flags". Resetting unmentioned prefs to defaults each boot keeps the join deterministic.
    up.arg("up")
        .arg("--reset")
        .arg(format!("--authkey={authkey}"))
        .arg(format!("--hostname={hostname}"));
    if let Some(tags) = &tags {
        up.arg(format!("--advertise-tags={tags}"));
    }
    // Best-effort: on a macOS GUI-app install the daemon is owned by the Tailscale app, so
    // CLI `up --authkey` can exit non-zero even though the node is (or can be) up. So we do
    // NOT bail on a non-zero `up` — we still try to `serve`, which succeeds whenever the node
    // is up however it got there. Only a missing CLI is fatal to bring-up.
    match run(up, "up") {
        Some(o) if o.status.success() => log::info!("tailscale up — hostname={hostname} tags={tags:?}"),
        Some(o) => log::warn!(
            "`tailscale up` exited with {} (daemon may be app-managed / not logged in) — still trying serve",
            o.status
        ),
        None => {
            log::warn!("`tailscale up` unavailable or hung; skipping tailnet setup this pass");
            return;
        }
    }

    // WAIT for the rename to settle before we serve + report. `tailscale up --hostname=X` renames
    // the node ASYNCHRONOUSLY (notably on the macOS GUI-managed daemon): reading `Self.DNSName`
    // immediately after `up` races the rename and can capture the OLD node name. That stale name
    // then gets `serve`d and reported to control as this box's live host — so the gateway resolves a
    // dead node and Live is a black screen while localhost:1984 works fine. This is THE live-video
    // bug. Blocking until the node reflects the requested hostname closes it.
    let settled = wait_for_hostname(&hostname, Duration::from_secs(20));

    // Expose the localhost-only go2rtc API over the tailnet (TLS via MagicDNS), and the edge API on
    // 8080 (443 is reserved for go2rtc so the gateway is unaffected).
    serve_go2rtc();
    serve_edge_api();

    // Report this box's tailnet host to control (so the gateway can resolve the tenant's live
    // upstream) — unless it was supplied explicitly at install time.
    if !host_is_explicit() {
        if let Some(dns) = settled.or_else(detect_dns_name) {
            set_public_host(&dns);
        }
    }
}

/// Was `EDGE_PUBLIC_HOST` an explicit install-time override (rather than something we auto-detected)?
/// If so we never touch it. Captured ONCE — the first call must happen before any auto-set, so
/// `ensure_up` primes it up front.
fn host_is_explicit() -> bool {
    static EXPLICIT: OnceLock<bool> = OnceLock::new();
    *EXPLICIT.get_or_init(|| {
        std::env::var("EDGE_PUBLIC_HOST")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

/// Set `EDGE_PUBLIC_HOST` from a MagicDNS name (idempotent; logs only on an actual change). The box
/// sends this as `X-Box-Host` on every config poll, so correcting it here corrects what the gateway
/// resolves within one poll.
fn set_public_host(dns: &str) {
    let url = format!("https://{}", dns.trim_end_matches('.'));
    if std::env::var("EDGE_PUBLIC_HOST").ok().as_deref() != Some(url.as_str()) {
        log::info!("EDGE_PUBLIC_HOST = {url}");
        std::env::set_var("EDGE_PUBLIC_HOST", &url);
    }
}

/// Poll `tailscale status` until `Self.DNSName`'s first label matches `hostname` — the `--hostname`
/// rename is asynchronous, so this is what stops us serving/reporting the previous node. Returns the
/// settled DNSName, or the last one seen at timeout (better a late guess than nothing).
fn wait_for_hostname(hostname: &str, timeout: Duration) -> Option<String> {
    let want = hostname.trim().to_ascii_lowercase();
    let deadline = Instant::now() + timeout;
    let mut last: Option<String> = None;
    loop {
        if let Some(dns) = detect_dns_name() {
            let first = dns.split('.').next().unwrap_or("").to_ascii_lowercase();
            last = Some(dns.clone());
            if first == want {
                return Some(dns);
            }
        }
        if Instant::now() >= deadline {
            if let Some(d) = &last {
                let got = d.split('.').next().unwrap_or("");
                log::warn!("tailscale node still '{got}' after {}s waiting for '{want}' — reporting it anyway", timeout.as_secs());
            }
            return last;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Re-assert tailnet exposure and correct a drifted host — called from the discovery loop so a box
/// that came up before its node settled, or lost its serve, heals ITSELF within minutes with no
/// restart and no terminal. This is what makes the live path self-recover for a real customer.
pub fn reassert() {
    if std::env::var("TS_AUTHKEY")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        return;
    }
    // A node that isn't Running can't serve anything, so re-running `serve` alone is useless: it
    // fails, the box reports `offline`, and the tailnet-side of the product (Live, snapshots, the
    // credential probe) stays dead. Until 0.5.42 that's exactly what happened — `reassert` only
    // re-ran `serve`, so a Tailscale that stopped for ANY reason (quit, reboot, OS update, a
    // daemon crash) left the box unreachable forever with no way back except re-enrolling. That's
    // a support call for something the box can fix by itself.
    if !node_running() {
        log::warn!("tailscale node is not Running — re-running `up` to rejoin the tailnet");
        ensure_up(); // up + wait for the rename + serve + report the host
        return;
    }
    serve_go2rtc();
    serve_edge_api();
    if !host_is_explicit() {
        if let Some(dns) = detect_dns_name() {
            set_public_host(&dns);
        }
    }
}

/// Is the local node joined and Running? (`BackendState` from `tailscale status --json`.)
/// Anything we can't read — CLI missing, daemon stopped, malformed JSON — counts as not running,
/// which is the safe reading: it's precisely when we want to re-assert `up`.
fn node_running() -> bool {
    let mut cmd = ts();
    cmd.args(["status", "--json"]);
    run(cmd, "status --json")
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| {
            v.get("BackendState")
                .and_then(|s| s.as_str())
                .map(|s| s == "Running")
        })
        .unwrap_or(false)
}

/// Expose the localhost-only go2rtc API (`:1984`) over the tailnet (TLS via MagicDNS).
/// Best-effort + idempotent — safe to re-run periodically; a no-op (logged at debug) when
/// Tailscale is stopped. Output is captured so a stopped daemon doesn't spam stderr.
pub fn serve_go2rtc() {
    let mut cmd = ts();
    cmd.args(["serve", "--bg", "1984"]);
    match run(cmd, "serve --bg 1984") {
        Some(o) if o.status.success() => log::debug!("tailscale serve --bg 1984 ok"),
        Some(o) => log::debug!(
            "tailscale serve --bg 1984: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        None => log::debug!("tailscale serve --bg 1984 unavailable or hung"),
    }
}

/// Expose the edge's own HTTP API (`:8788`) over the tailnet on port 8080.
/// Workflow nodes (recording, snapshots) call https://<magicdns>:8080/ to reach the API.
/// Port 443 is reserved for go2rtc so the gateway is unaffected.
pub fn serve_edge_api() {
    let mut cmd = ts();
    cmd.args(["serve", "--bg", "--https=8080", "8788"]);
    match run(cmd, "serve --https=8080 8788") {
        Some(o) if o.status.success() => {
            log::info!("tailscale serve --https=8080 8788 ok (edge API on tailnet)")
        }
        Some(o) => log::warn!(
            "tailscale serve --https=8080 8788: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        None => log::warn!("tailscale serve (edge API) unavailable or hung"),
    }
}

/// A coarse Tailscale health string for the cloud Boxes page:
///   `disabled` — no `TS_AUTHKEY` (LAN/dev box, tailnet not used)
///   `offline`  — CLI missing or the node isn't `Running` (Live can't work)
///   `up`       — node Running but go2rtc isn't served over the tailnet yet
///   `serving`  — node Running AND go2rtc exposed (Live should work)
pub fn tailscale_state() -> &'static str {
    if std::env::var("TS_AUTHKEY")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        return "disabled";
    }
    if !node_running() {
        return "offline";
    }
    let mut cmd = ts();
    cmd.args(["serve", "status"]);
    let serving = run(cmd, "serve status")
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("1984"))
        .unwrap_or(false);
    if serving {
        "serving"
    } else {
        "up"
    }
}

/// This node's MagicDNS name from `tailscale status --json` (`Self.DNSName`), if up.
fn detect_dns_name() -> Option<String> {
    let mut cmd = ts();
    cmd.args(["status", "--json"]);
    let out = run(cmd, "status --json")?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("Self")?
        .get("DNSName")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
