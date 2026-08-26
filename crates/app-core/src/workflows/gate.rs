//! What happens when a graph is asked to run while one of its runs is already going.
//!
//! Nothing here executes a graph. This decides only whether a fire becomes a run at all: a second
//! signal can start alongside, wait its turn, or be dropped. It lived next to the old scheduler
//! for no better reason than being written on the same day.

use super::domain::ConcurrencyPolicy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fire {
    Spawn,
    Queue,
    Skip,
}

#[derive(Default, Clone, Copy)]
struct GraphRun {
    running: u32,
    queued: u32,
}

/// Per-(tenant, graph) concurrency counters + per-tenant new-alert high-water mark. Keyed by
/// `(tenant_id, graph_id)` because graph ids are per-tenant — a single pod serving multiple tenants
/// would otherwise collide graph 1 of tenant A with graph 1 of tenant B.
///
/// It used to hold `cron_last`, the in-memory anchor for graph-level `Trigger::Cron`. That went
/// with the trigger (migration `0095`): a cron NODE's next fire is a durable `work_jobs` row, so
/// nothing has to be reconstructed at boot to keep a schedule honest.
#[derive(Default)]
pub struct WorkflowEngine {
    runs: HashMap<(uuid::Uuid, uuid::Uuid), GraphRun>,
    alert_seen_max: HashMap<uuid::Uuid, u64>,
}

impl WorkflowEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a tenant's new-alert high-water mark at boot (from the current alert-store max)
    /// so the existing backlog doesn't fire `OnNewAlert` graphs on startup.
    pub fn seed_alert_seen_max(&mut self, tenant: uuid::Uuid, max: u64) {
        self.alert_seen_max.insert(tenant, max);
    }

    /// Decide whether a fresh fire of `(tenant, graph_id)` should spawn, queue, or be skipped.
    pub fn decide(
        &self,
        tenant: uuid::Uuid,
        graph_id: uuid::Uuid,
        policy: &ConcurrencyPolicy,
    ) -> Fire {
        let r = self
            .runs
            .get(&(tenant, graph_id))
            .copied()
            .unwrap_or_default();
        match policy {
            ConcurrencyPolicy::SkipIfRunning => {
                if r.running == 0 {
                    Fire::Spawn
                } else {
                    Fire::Skip
                }
            }
            ConcurrencyPolicy::QueueSingle { max_queued } => {
                if r.running == 0 {
                    Fire::Spawn
                } else if r.queued < *max_queued {
                    Fire::Queue
                } else {
                    Fire::Skip
                }
            }
            ConcurrencyPolicy::Concurrent { max_concurrent } => {
                if r.running < (*max_concurrent).max(1) {
                    Fire::Spawn
                } else {
                    Fire::Skip
                }
            }
        }
    }

    pub fn on_spawn(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid) {
        self.runs.entry((tenant, graph_id)).or_default().running += 1;
    }

    pub fn on_queue(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid) {
        self.runs.entry((tenant, graph_id)).or_default().queued += 1;
    }

    /// Call when a run finishes. Returns `true` if a queued fire should now spawn.
    pub fn on_finish(&mut self, tenant: uuid::Uuid, graph_id: uuid::Uuid) -> bool {
        let r = self.runs.entry((tenant, graph_id)).or_default();
        if r.running > 0 {
            r.running -= 1;
        }
        if r.queued > 0 {
            r.queued -= 1;
            true
        } else {
            false
        }
    }

    // NOTE: no `total_running()` here on purpose. `runs` is per-graph policy accounting —
    // only `fire_graph` (cron/manual) feeds it via `on_spawn`, so a sum of it silently omits
    // every event handler. The pod-wide backstop counts the activity map instead
    // (`sentinel_flow::engine::total_in_flight`), which both spawn paths register with.

    /// A tenant's current new-alert high-water mark (0 if unseen) — so the adapter can
    /// detect when a tick advanced it and persist the new value.
    pub fn alert_seen_max(&self, tenant: uuid::Uuid) -> u64 {
        self.alert_seen_max.get(&tenant).copied().unwrap_or(0)
    }
}
