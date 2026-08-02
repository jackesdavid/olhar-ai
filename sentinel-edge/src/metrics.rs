//! Edge resource-usage sampling — CPU / memory / disk via `sysinfo`, GPU via `nvidia-smi`.
//!
//! A background thread refreshes a cached [`EdgeMetrics`] every [`SAMPLE_INTERVAL`]; the
//! heartbeat (`state::with_health`) reads the latest snapshot and ships it as the compact
//! `X-Edge-Metrics` JSON header. Sampling on a timer (not per-heartbeat) keeps CPU% accurate —
//! `sysinfo` needs two refreshes spaced apart to compute utilization — and off the request path.

use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use app_core::edge_metrics::EdgeMetrics;
use sysinfo::{Disks, System};

/// How often the background thread re-samples. Also the CPU-usage measurement window.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(10);

fn cache() -> &'static RwLock<EdgeMetrics> {
    static CACHE: OnceLock<RwLock<EdgeMetrics>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(EdgeMetrics::default()))
}

/// The latest sampled snapshot (all-zero until the first sample lands).
pub fn current() -> EdgeMetrics {
    *cache().read().unwrap()
}

/// The value for the `X-Edge-Metrics` heartbeat header (compact JSON, ASCII).
pub fn header_value() -> String {
    current().to_header()
}

/// Start the background sampler. Idempotent-ish: call once at boot.
pub fn spawn() {
    std::thread::Builder::new()
        .name("edge-metrics".into())
        .spawn(|| {
            let mut sys = System::new();
            // Prime CPU counters so the first real sample isn't a bogus 0% (sysinfo needs a
            // prior refresh to diff against).
            sys.refresh_cpu();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            loop {
                let m = sample(&mut sys);
                *cache().write().unwrap() = m;
                std::thread::sleep(SAMPLE_INTERVAL);
            }
        })
        .expect("spawn edge-metrics thread");
}

fn sample(sys: &mut System) -> EdgeMetrics {
    sys.refresh_cpu();
    sys.refresh_memory();

    let cpu = sys.global_cpu_info().cpu_usage().round().clamp(0.0, 100.0) as u8;

    let total = sys.total_memory();
    let mem = if total == 0 {
        0
    } else {
        ((sys.used_memory().saturating_mul(100)) / total).min(100) as u8
    };

    let disk = disk_pct();
    let (gpu, gpu_mem) = gpu_pct();

    EdgeMetrics {
        cpu,
        mem,
        disk,
        gpu,
        gpu_mem,
    }
}

/// Used-percent of the volume that actually holds the recordings (card #162).
///
/// This used to pin to `/` and fall back to the largest disk. On macOS `/` is the SEALED,
/// READ-ONLY system volume, so a box reported `disk=0%` while `/System/Volumes/Data` was 100%
/// full — the health gauge said healthy about a box that could not write a clip. Resolving the
/// recordings directory to its own mount fixes that case and the Linux separate-`/data`-disk case
/// with the same rule.
///
/// Falls back to `/` only when the recordings path has not been registered yet (before
/// [`set_data_dir`] runs at boot).
fn disk_pct() -> u8 {
    let dir = data_dir()
        .read()
        .unwrap()
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    let target = std::fs::canonicalize(&dir).unwrap_or(dir);
    let disks = Disks::new_with_refreshed_list();
    // Longest matching mount point: `/` is a prefix of every path, so a first-match search reports
    // the wrong volume for anything on a separate disk — and reports it as healthy.
    let holding = disks
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .or_else(|| disks.iter().max_by_key(|d| d.total_space()));
    match holding {
        Some(d) if d.total_space() > 0 => {
            let used = d.total_space().saturating_sub(d.available_space());
            ((used.saturating_mul(100)) / d.total_space()).min(100) as u8
        }
        _ => 0,
    }
}

/// The recordings directory, registered at boot so `disk_pct` measures the volume the box actually
/// writes to rather than whatever is mounted at `/`.
fn data_dir() -> &'static std::sync::RwLock<Option<std::path::PathBuf>> {
    static DIR: std::sync::OnceLock<std::sync::RwLock<Option<std::path::PathBuf>>> =
        std::sync::OnceLock::new();
    DIR.get_or_init(|| std::sync::RwLock::new(None))
}

/// Tell the sampler which volume matters. Call before [`spawn`].
pub fn set_data_dir(dir: std::path::PathBuf) {
    *data_dir().write().unwrap() = Some(dir);
}

/// GPU utilization + memory-used percent via `nvidia-smi`. `(None, None)` when no NVIDIA GPU
/// (command missing / non-zero exit / unparsable) — the UI then hides the GPU gauge.
fn gpu_pct() -> (Option<u8>, Option<u8>) {
    let out = crate::proc::command("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(out) = out else { return (None, None) };
    if !out.status.success() {
        return (None, None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(line) = text.lines().next() else {
        return (None, None);
    };
    let mut cols = line.split(',').map(|c| c.trim());
    let util = cols.next().and_then(|v| v.parse::<f32>().ok());
    let used = cols.next().and_then(|v| v.parse::<f64>().ok());
    let total = cols.next().and_then(|v| v.parse::<f64>().ok());
    let gpu = util.map(|v| v.round().clamp(0.0, 100.0) as u8);
    let gpu_mem = match (used, total) {
        (Some(u), Some(t)) if t > 0.0 => Some((u / t * 100.0).round().clamp(0.0, 100.0) as u8),
        _ => None,
    };
    (gpu, gpu_mem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_sane_values_and_header_roundtrips() {
        let mut sys = System::new();
        sys.refresh_cpu();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let m = sample(&mut sys);

        // Percentages are whole 0..=100 (u8 already bounds ≤255; assert the clamp holds).
        assert!(m.cpu <= 100, "cpu out of range: {}", m.cpu);
        assert!(m.mem <= 100, "mem out of range: {}", m.mem);
        assert!(m.disk <= 100, "disk out of range: {}", m.disk);
        if let Some(g) = m.gpu {
            assert!(g <= 100);
        }
        // A real machine has some memory in use and a non-empty root disk.
        assert!(m.mem > 0, "expected non-zero memory usage");
        assert!(m.disk > 0, "expected non-zero disk usage");

        // The compact header JSON must roundtrip through control's parse path.
        let back = EdgeMetrics::from_header(&m.to_header()).expect("header parses");
        assert_eq!(back.cpu, m.cpu);
        assert_eq!(back.mem, m.mem);
        assert_eq!(back.disk, m.disk);
        assert_eq!(back.gpu, m.gpu);
    }
}
