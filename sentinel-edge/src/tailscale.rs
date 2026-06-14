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
    let tags = std::env::var("TS_TAGS").ok().filter(|s| !s.trim().is_empty());

    let mut up = Command::new("tailscale");
    up.arg("up")
        .arg(format!("--authkey={authkey}"))
        .arg(format!("--hostname={hostname}"));
    if let Some(tags) = &tags {
        up.arg(format!("--advertise-tags={tags}"));
    }
    match up.status() {
        Ok(s) if s.success() => log::info!("tailscale up — hostname={hostname} tags={tags:?}"),
        Ok(s) => {
            log::warn!("`tailscale up` exited with {s}; continuing without the tailnet");
            return;
        }
        Err(e) => {
            log::warn!("`tailscale` not available ({e}); continuing without the tailnet");
            return;
        }
    }

    // Expose the localhost-only go2rtc API over the tailnet (TLS via MagicDNS).
    match Command::new("tailscale").args(["serve", "--bg", "1984"]).status() {
        Ok(s) if s.success() => {
            log::info!("tailscale serve --bg 1984 → https://{hostname}.<tailnet>.ts.net")
        }
        Ok(s) => log::warn!("`tailscale serve` exited with {s}"),
        Err(e) => log::warn!("`tailscale serve` failed: {e}"),
    }
}
