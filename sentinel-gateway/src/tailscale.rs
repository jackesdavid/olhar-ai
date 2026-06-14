//! Tailscale bring-up for the gateway. The gateway is a tailnet member so it can
//! reach edge boxes at their MagicDNS names over the private network. Opt-in via
//! `TS_AUTHKEY`; otherwise the host is assumed already joined (or it's one-box dev
//! over loopback). Best-effort — never fatal.
use std::process::Command;

/// If `TS_AUTHKEY` is set, join the tailnet (`tailscale up`). Idempotent.
pub fn ensure_up() {
    let Ok(authkey) = std::env::var("TS_AUTHKEY") else {
        log::info!("TS_AUTHKEY unset — skipping Tailscale (assuming host already on the tailnet / dev)");
        return;
    };
    if authkey.trim().is_empty() {
        return;
    }
    let hostname = std::env::var("TS_HOSTNAME").unwrap_or_else(|_| "sentinel-gateway".to_string());
    let mut up = Command::new("tailscale");
    up.arg("up").arg(format!("--authkey={authkey}")).arg(format!("--hostname={hostname}"));
    if let Some(tags) = std::env::var("TS_TAGS").ok().filter(|s| !s.trim().is_empty()) {
        up.arg(format!("--advertise-tags={tags}"));
    }
    match up.status() {
        Ok(s) if s.success() => log::info!("tailscale up — gateway joined as {hostname}"),
        Ok(s) => log::warn!("`tailscale up` exited with {s}; continuing"),
        Err(e) => log::warn!("`tailscale` not available ({e}); continuing"),
    }
}
