//! Reusable device actions, copied from the ratatui `src/device/actions.rs`:
//! listing, name resolution, snapshot capture, and deletion. The terminal-only
//! `decode_luma` / `capture_snapshot` (which pulled in the `image` crate) are
//! dropped — the web shows JPEG bytes directly. Persistence is no longer baked
//! in here: the deletion/rename helpers mutate a `&mut Vec<Device>` and the
//! Tauri layer persists to its own path after the mutation.
use std::fmt;

use crate::devices::Device;
use crate::onvif;

/// A compact view of a device for listing.
pub struct DeviceSummary {
    pub name: String,
    pub ip: String,
    pub has_credentials: bool,
}

pub fn list_summaries(devices: &[Device]) -> Vec<DeviceSummary> {
    devices
        .iter()
        .map(|d| DeviceSummary {
            name: d.name.clone(),
            ip: d.ip.clone(),
            has_credentials: d.has_credentials(),
        })
        .collect()
}

/// Plain-text rendering of the device list.
pub fn summaries_text(summaries: &[DeviceSummary]) -> String {
    if summaries.is_empty() {
        return "No devices on file.".into();
    }
    let mut out = String::new();
    for (i, d) in summaries.iter().enumerate() {
        let creds = if d.has_credentials { "credentials saved" } else { "no credentials" };
        out.push_str(&format!("{}. {} ({}) — {}\n", i + 1, d.name, d.ip, creds));
    }
    out.trim_end().to_string()
}

/// Failure to resolve a device by a free-form query.
#[derive(Debug)]
pub enum ResolveError {
    NotFound(String),
    Ambiguous(String, Vec<String>),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::NotFound(q) => write!(f, "No device named '{q}'."),
            ResolveError::Ambiguous(q, names) => {
                write!(f, "Multiple devices match '{q}': {}. Which one?", names.join(", "))
            }
        }
    }
}

/// Resolve a device index by query: `"first"`, exact name (case-insensitive), or
/// exact IP. Strict on ambiguity so the caller asks rather than guesses.
pub fn resolve_device(devices: &[Device], query: &str) -> Result<usize, ResolveError> {
    let q = query.trim();
    if q.eq_ignore_ascii_case("first") {
        return if devices.is_empty() {
            Err(ResolveError::NotFound(q.into()))
        } else {
            Ok(0)
        };
    }
    let matches: Vec<usize> = devices
        .iter()
        .enumerate()
        .filter(|(_, d)| d.name.eq_ignore_ascii_case(q) || d.ip == q)
        .map(|(i, _)| i)
        .collect();
    match matches.len() {
        0 => Err(ResolveError::NotFound(q.into())),
        1 => Ok(matches[0]),
        _ => Err(ResolveError::Ambiguous(
            q.into(),
            matches.iter().map(|&i| devices[i].name.clone()).collect(),
        )),
    }
}

/// Capture a snapshot from a camera and return the raw encoded JPEG bytes. The web
/// app base64-encodes these for an `<img>` data URL.
///
/// Fast path: grab the current frame from the shared go2rtc feed (one persistent
/// RTSP connection per camera, already used by the Live page) — ~tens of ms when
/// warm. Falls back to a from-scratch ONVIF/ffmpeg capture (slow but always works)
/// when the sidecar isn't running or the stream isn't ready.
pub fn capture_snapshot_bytes(d: &Device) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = crate::go2rtc::frame(&d.ip) {
        return Ok(bytes);
    }
    if !d.has_credentials() {
        return Err(format!("Camera '{}' has no saved credentials.", d.name));
    }
    let user = d.username.clone().unwrap_or_default();
    let pass = d.password.clone().unwrap_or_default();
    let rtsp = d.rtsp_url.clone().unwrap_or_default();
    onvif::client::snapshot(&d.xaddr, &user, &pass, &rtsp)
}

/// Delete a device by name (case-insensitive) or IP from `devices`. Returns the
/// removed device's name. The caller persists afterward.
pub fn delete_by_name(devices: &mut Vec<Device>, name: &str) -> Result<String, String> {
    let idx = resolve_device(devices, name).map_err(|e| e.to_string())?;
    Ok(devices.remove(idx).name)
}

/// Delete a device by its 1-based position in `devices` (1 = first). Returns the
/// removed device's name. The caller persists afterward.
pub fn delete_by_index(devices: &mut Vec<Device>, index: usize) -> Result<String, String> {
    if index == 0 || index > devices.len() {
        return Err(format!("No device at position {index} (there are {}).", devices.len()));
    }
    Ok(devices.remove(index - 1).name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(name: &str, ip: &str, creds: bool) -> Device {
        Device {
            id: String::new(),
            ip: ip.into(),
            xaddr: String::new(),
            name: name.into(),
            manufacturer: None,
            model: None,
            username: creds.then(|| "u".to_string()),
            password: creds.then(|| "p".to_string()),
            rtsp_url: None,
            rtsp_sub_url: None,
        }
    }

    #[test]
    fn resolve_first_exact_ip_ambiguous_notfound() {
        let d = vec![mk("Front", "10.0.0.1", true), mk("Back", "10.0.0.2", false), mk("Front", "10.0.0.3", false)];
        assert_eq!(resolve_device(&d, "first").unwrap(), 0);
        assert_eq!(resolve_device(&d, "back").unwrap(), 1); // case-insensitive
        assert_eq!(resolve_device(&d, "10.0.0.3").unwrap(), 2); // by IP
        assert!(matches!(resolve_device(&d, "Front"), Err(ResolveError::Ambiguous(..))));
        assert!(matches!(resolve_device(&d, "nope"), Err(ResolveError::NotFound(_))));
    }

    #[test]
    fn delete_by_index_removes_and_returns_name() {
        let mut d = vec![mk("Front", "10.0.0.1", true), mk("Back", "10.0.0.2", false)];
        assert_eq!(delete_by_index(&mut d, 1).unwrap(), "Front");
        assert_eq!(d.len(), 1);
        assert!(delete_by_index(&mut d, 5).is_err());
    }

    #[test]
    fn summaries_text_lists_creds_status() {
        let s = list_summaries(&[mk("Front", "10.0.0.1", true), mk("Back", "10.0.0.2", false)]);
        let t = summaries_text(&s);
        assert!(t.contains("Front (10.0.0.1) — credentials saved"));
        assert!(t.contains("Back (10.0.0.2) — no credentials"));
    }
}
