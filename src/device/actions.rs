//! Reusable device actions, shared by the chat tools and the manual UI: listing,
//! name resolution, snapshot capture, and deletion. Keeping them here means the
//! LLM tools and the keyboard-driven UI invoke the exact same logic.
use std::fmt;

use image::GrayImage;

use crate::onvif;
use crate::scan::domain::save_devices;
use crate::scan::{Device, DeviceStore};

/// A compact view of a device for listing in chat / the tool result.
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

/// Plain-text rendering of the device list (used as the tool_result content).
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
/// exact IP. Strict on ambiguity so the assistant asks rather than guesses.
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

/// Capture a snapshot from a camera and return the raw encoded bytes (JPEG over
/// HTTP, or a decoded frame from the RTSP fallback). Requires saved credentials.
/// This is the shared source for both monochrome display and vision description,
/// so the camera is hit only once.
pub fn capture_snapshot_bytes(d: &Device) -> Result<Vec<u8>, String> {
    if !d.has_credentials() {
        return Err(format!("Camera '{}' has no saved credentials.", d.name));
    }
    let user = d.username.clone().unwrap_or_default();
    let pass = d.password.clone().unwrap_or_default();
    let rtsp = d.rtsp_url.clone().unwrap_or_default();
    onvif::client::snapshot(&d.xaddr, &user, &pass, &rtsp)
}

/// Decode snapshot bytes to grayscale, ready for monochrome rendering.
pub fn decode_luma(bytes: &[u8]) -> Result<GrayImage, String> {
    image::load_from_memory(bytes)
        .map(|img| img.to_luma8())
        .map_err(|e| format!("Could not decode image: {e}"))
}

/// Capture a snapshot and decode it to grayscale in one step.
pub fn capture_snapshot(d: &Device) -> Result<GrayImage, String> {
    decode_luma(&capture_snapshot_bytes(d)?)
}

/// Delete a device by name (case-insensitive) or IP, and persist. Returns the
/// removed device's name.
pub fn delete_by_name(store: &DeviceStore, name: &str) -> Result<String, String> {
    let mut devices = store.lock().unwrap();
    let idx = resolve_device(&devices, name).map_err(|e| e.to_string())?;
    let removed = devices.remove(idx);
    save_devices(&devices);
    Ok(removed.name)
}

/// Delete a device by its 1-based position in the list (1 = first), and persist.
/// Returns the removed device's name.
pub fn delete_by_index(store: &DeviceStore, index: usize) -> Result<String, String> {
    let mut devices = store.lock().unwrap();
    if index == 0 || index > devices.len() {
        return Err(format!("No device at position {index} (there are {}).", devices.len()));
    }
    let removed = devices.remove(index - 1);
    save_devices(&devices);
    Ok(removed.name)
}

/// Rename the device with the given IP, and persist. Returns the new name.
pub fn rename_device(store: &DeviceStore, ip: &str, new_name: &str) -> Result<String, String> {
    let name = new_name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty.".into());
    }
    let mut devices = store.lock().unwrap();
    let device = devices
        .iter_mut()
        .find(|d| d.ip == ip)
        .ok_or("Device not found.")?;
    device.name = name.to_string();
    let renamed = device.name.clone();
    save_devices(&devices);
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(name: &str, ip: &str, creds: bool) -> Device {
        Device {
            ip: ip.into(),
            xaddr: String::new(),
            name: name.into(),
            manufacturer: None,
            model: None,
            username: creds.then(|| "u".to_string()),
            password: creds.then(|| "p".to_string()),
            rtsp_url: None,
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
    fn summaries_text_lists_creds_status() {
        let s = list_summaries(&[mk("Front", "10.0.0.1", true), mk("Back", "10.0.0.2", false)]);
        let t = summaries_text(&s);
        assert!(t.contains("Front (10.0.0.1) — credentials saved"));
        assert!(t.contains("Back (10.0.0.2) — no credentials"));
    }
}
