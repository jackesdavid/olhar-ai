//! Shared edge state + go2rtc lifecycle. The device list is the edge's view of which
//! cameras exist (read from the shared Postgres); go2rtc holds the persistent RTSP
//! connections it grabs frames from.
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};

use app_core::db::Db;
use app_core::devices::{self, Device};

/// Everything the HTTP handlers + go2rtc manager share.
pub struct AppState {
    pub db: Db,
    /// The edge's device list (resolved by ip on each snapshot), refreshed from
    /// Postgres at boot and on `POST /reload`.
    pub devices: Mutex<Vec<Device>>,
    /// The managed go2rtc child (when the edge owns the sidecar — `EDGE_OWN_GO2RTC`).
    pub go2rtc: Mutex<Option<Child>>,
    /// Whether the edge owns/spawns go2rtc (vs. using one already on :1984).
    pub own_go2rtc: bool,
    /// Where to write the generated go2rtc config + find the binary.
    pub go2rtc_config: PathBuf,
    pub go2rtc_bin: PathBuf,
}

impl AppState {
    /// Connect to Postgres (applies migrations) and hydrate the device list.
    pub fn boot(database_url: &str) -> Result<Arc<AppState>, String> {
        let db = Db::connect(database_url).map_err(|e| format!("connect Postgres: {e}"))?;
        log::info!("Postgres connected; migrations applied");
        let devices = devices::repo::load(&db);
        log::info!("Loaded {} device(s) from Postgres", devices.len());

        // The edge IS the camera host, so it owns go2rtc by default. Set
        // EDGE_OWN_GO2RTC=0 only when another process on the box already serves :1984
        // (e.g. running alongside the desktop app in dev).
        let own_go2rtc = std::env::var("EDGE_OWN_GO2RTC")
            .map(|v| !matches!(v.trim(), "0" | "false" | "no"))
            .unwrap_or(true);
        let go2rtc_config = std::env::var("GO2RTC_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("go2rtc.yaml"));
        let go2rtc_bin = std::env::var("GO2RTC_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("go2rtc"));

        let state = Arc::new(AppState {
            db,
            devices: Mutex::new(devices),
            go2rtc: Mutex::new(None),
            own_go2rtc,
            go2rtc_config,
            go2rtc_bin,
        });
        Ok(state)
    }

    /// Resolve a saved device by exact IP.
    pub fn device_by_ip(&self, ip: &str) -> Option<Device> {
        self.devices.lock().unwrap().iter().find(|d| d.ip == ip).cloned()
    }

    /// Re-read the device list from Postgres (and, if owned, restart go2rtc to match).
    pub fn reload(&self) -> usize {
        let fresh = devices::repo::load(&self.db);
        let n = fresh.len();
        *self.devices.lock().unwrap() = fresh;
        if self.own_go2rtc {
            crate::go2rtc::respawn(self);
        }
        n
    }
}
