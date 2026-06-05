//! Scan-feature behaviour: advancing the sonar sweep, selection, the
//! deduplicated merge of discovered devices, and persistence.
use super::domain::{save_devices, Device, ScanState, Tracker, SWEEP_SPEED, TAU};

impl Tracker {
    /// Advance the rotating sweep one tick.
    pub fn tick(&mut self) {
        self.sweep = (self.sweep + SWEEP_SPEED) % TAU;
    }
}

impl ScanState {
    /// Advance the sonar (kept ticking for the global animation phase; the view
    /// decides whether to actually draw the moving sweep).
    pub fn tick(&mut self) {
        self.tracker.tick();
    }

    pub fn next(&mut self) {
        let n = self.device_count();
        if n > 0 {
            self.selected = (self.selected + 1) % n;
        }
    }

    pub fn prev(&mut self) {
        let n = self.device_count();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }

    /// Keep the selection in range after a removal.
    pub fn clamp_selection(&mut self) {
        let n = self.device_count();
        self.selected = self.selected.min(n.saturating_sub(1));
    }

    /// Merge freshly discovered devices, skipping any IP we already have.
    /// Returns how many were newly added.
    pub fn merge(&mut self, found: Vec<Device>) -> usize {
        let mut devices = self.devices.lock().unwrap();
        let mut added = 0;
        for d in found {
            if !devices.iter().any(|e| e.ip == d.ip) {
                devices.push(d);
                added += 1;
            }
        }
        added
    }

    pub fn save(&self) {
        save_devices(&self.devices.lock().unwrap());
    }
}
