//! Where the box keeps its config and data, per platform.
//!
//! On Unix these are the historical fixed locations (`/etc/sentinel-edge` for config,
//! `/opt/sentinel-edge` for data). On Windows there is no `/etc` or `/opt`; the convention for a
//! machine-wide service's state is `%ProgramData%` (`C:\ProgramData\Sentinel`), which every user
//! can't casually write but the service (running as a system account) can.
//!
//! These are only **defaults** — the installer overrides each via env (`EDGE_ENV_FILE`,
//! `EDGE_RECORDING_DIR`, `EDGE_CACHE`, …), exactly as the macOS `.pkg` does. They matter for a bare
//! run, and for the Windows installer (#83) to have a sane place to point at.

use std::path::PathBuf;

/// The machine-wide base directory for everything this box owns.
///
/// Unix keeps config (`/etc`) and data (`/opt`) split by tradition; Windows puts both under one
/// `%ProgramData%\Sentinel`, which is how Windows services are expected to store shared state.
#[cfg(windows)]
fn base() -> PathBuf {
    // %ProgramData% is C:\ProgramData on a default install; fall back to it literally if the env
    // var is somehow unset (it is set on every supported Windows).
    let root = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    root.join("Sentinel")
}

/// iOS has no `/etc` or `/opt` and its filesystem is read-only outside the app sandbox (card #144).
/// Everything lives under one writable base — the native module may set `EDGE_DATA_DIR` to a durable
/// sandbox dir (Application Support); otherwise the app's tmp dir, which is enough for a session.
#[cfg(target_os = "ios")]
fn base() -> PathBuf {
    std::env::var_os("EDGE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("sentinel-edge"))
}

/// The `edge.env` path: `%ProgramData%\Sentinel\edge.env` on Windows, `/etc/sentinel-edge/edge.env`
/// on Unix.
pub fn env_file() -> PathBuf {
    #[cfg(windows)]
    {
        base().join("edge.env")
    }
    #[cfg(target_os = "ios")]
    {
        base().join("edge.env")
    }
    #[cfg(all(not(windows), not(target_os = "ios")))]
    {
        PathBuf::from("/etc/sentinel-edge/edge.env")
    }
}

/// The camera cache file — the edge's durable, only home for camera credentials. Historically a
/// bare relative path (`cameras-cache.json`) in the service's CWD on the appliance; on iOS the CWD
/// is not writable, so it lands under the sandbox base (card #144), where creds also persist across
/// app launches instead of being re-staged every cold boot.
pub fn cache_file() -> PathBuf {
    #[cfg(target_os = "ios")]
    {
        base().join("cameras-cache.json")
    }
    #[cfg(not(target_os = "ios"))]
    {
        PathBuf::from("cameras-cache.json")
    }
}

/// The recordings directory: `%ProgramData%\Sentinel\recordings` on Windows,
/// `/opt/sentinel-edge/recordings` on Unix.
pub fn recordings_dir() -> PathBuf {
    #[cfg(windows)]
    {
        base().join("recordings")
    }
    #[cfg(target_os = "ios")]
    {
        base().join("recordings")
    }
    #[cfg(all(not(windows), not(target_os = "ios")))]
    {
        PathBuf::from("/opt/sentinel-edge/recordings")
    }
}
