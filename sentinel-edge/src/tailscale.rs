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
use std::process::Command;

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
    let hostname = std::env::var("TS_HOSTNAME").unwrap_or_else(|_| "sentinel-edge".to_string());
    let tags = std::env::var("TS_TAGS")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let mut up = Command::new("tailscale");
    up.arg("up")
        .arg(format!("--authkey={authkey}"))
        .arg(format!("--hostname={hostname}"));
    if let Some(tags) = &tags {
        up.arg(format!("--advertise-tags={tags}"));
    }
    // Best-effort: on a macOS GUI-app install the daemon is owned by the Tailscale app, so
    // CLI `up --authkey` can exit non-zero even though the node is (or can be) up. So we do
    // NOT bail on a non-zero `up` — we still try to `serve`, which succeeds whenever the node
    // is up however it got there. Only a missing CLI is fatal to bring-up.
    match up.status() {
        Ok(s) if s.success() => log::info!("tailscale up — hostname={hostname} tags={tags:?}"),
        Ok(s) => log::warn!(
            "`tailscale up` exited with {s} (daemon may be app-managed / not logged in) — still trying serve"
        ),
        Err(e) => {
            log::warn!("`tailscale` CLI not available ({e}); skipping tailnet setup");
            return;
        }
    }

    // Expose the localhost-only go2rtc API over the tailnet (TLS via MagicDNS).
    serve_go2rtc();

    // Report this box's tailnet go2rtc URL to control (so the gateway can resolve the
    // tenant's live upstream) — unless it was supplied explicitly at install time.
    if std::env::var("EDGE_PUBLIC_HOST")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        if let Some(dns) = detect_dns_name() {
            let url = format!("https://{}", dns.trim_end_matches('.'));
            log::info!("detected EDGE_PUBLIC_HOST={url}");
            std::env::set_var("EDGE_PUBLIC_HOST", url);
        }
    }
}

/// Expose the localhost-only go2rtc API (`:1984`) over the tailnet (TLS via MagicDNS).
/// Best-effort + idempotent — safe to re-run periodically; a no-op (logged at debug) when
/// Tailscale is stopped. Output is captured so a stopped daemon doesn't spam stderr.
pub fn serve_go2rtc() {
    match Command::new("tailscale")
        .args(["serve", "--bg", "1984"])
        .output()
    {
        Ok(o) if o.status.success() => log::debug!("tailscale serve --bg 1984 ok"),
        Ok(o) => log::debug!(
            "tailscale serve --bg 1984: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => log::debug!("tailscale serve unavailable: {e}"),
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
    let running = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| {
            v.get("BackendState")
                .and_then(|s| s.as_str())
                .map(|s| s == "Running")
        })
        .unwrap_or(false);
    if !running {
        return "offline";
    }
    let serving = Command::new("tailscale")
        .args(["serve", "status"])
        .output()
        .ok()
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
    let out = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()?;
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
