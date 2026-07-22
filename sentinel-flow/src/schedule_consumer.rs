//! Durable calendar-schedule trigger consumer (#111). Each enabled `Trigger::Schedule` graph has ONE
//! pending `work_jobs` row whose `run_after` is its next fire (computed in the graph's timezone).
//! The consumer claims due rows → fires the graph (as a `Cron` source) → reschedules the same row to
//! the following occurrence. [`reconcile`] rebuilds the rows from the current enabled graphs — called
//! on boot and on every graph reload — so schedule edits / enable / disable take effect at once.
//!
//! Option B (queue) from the plan: calendar math stays out of the 1s poller, and an armed schedule
//! survives a pod restart (the next fire is a row in Postgres, not a held thread).
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use app_core::work_queue;
use app_core::workflows::domain::{Trigger, TriggerSource};
use app_core::workflows::schedule::next_occurrence;

use crate::engine::fire_graph;
use crate::state::{self, AppState};

pub const SCHEDULE_CATEGORY: &str = "workflow_schedule";
pub const SCHEDULE_KIND: &str = "fire";

const IDLE_POLL: Duration = Duration::from_millis(500);
const BATCH: i64 = 50;

/// Rebuild the pending schedule jobs from the current enabled `Schedule` graphs. Deletes the pending
/// ones and re-enqueues one per graph at its next occurrence. Idempotent (`next_occurrence` is
/// deterministic for a given schedule + now), so it's safe to call on every reload.
pub fn reconcile(state: &AppState) {
    work_queue::delete_pending_by_category_blocking(&state.db, SCHEDULE_CATEGORY);
    let now = Utc::now();
    let jobs: Vec<(Uuid, Uuid, String)> = {
        let s = state.store.lock().unwrap();
        s.iter()
            .filter(|(_, g)| g.enabled)
            .filter_map(|(t, g)| match &g.trigger {
                Trigger::Schedule(sched) => next_occurrence(sched, now).map(|n| (*t, g.id, n.to_rfc3339())),
                _ => None,
            })
            .collect()
    };
    let n = jobs.len();
    for (tenant, graph_id, run_after) in jobs {
        let payload = json!({ "graph": graph_id.to_string() });
        work_queue::enqueue_at_blocking(&state.db, tenant, SCHEDULE_CATEGORY, SCHEDULE_KIND, payload, &run_after);
    }
    if n > 0 {
        log::info!("workflow schedules reconciled: {n} armed");
    }
}

pub fn spawn(state: Arc<AppState>) {
    reconcile(&state); // arm existing schedules on boot
    std::thread::spawn(move || loop {
        let jobs = work_queue::claim_blocking(&state.db, SCHEDULE_CATEGORY, SCHEDULE_KIND, BATCH);
        if jobs.is_empty() {
            std::thread::sleep(IDLE_POLL);
            continue;
        }
        for job in jobs {
            process(&state, job);
        }
    });
    log::info!("workflow schedule consumer started (work_jobs)");
}

fn process(state: &Arc<AppState>, job: work_queue::Job) {
    let Some(graph_id) = job.payload.get("graph").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()) else {
        work_queue::mark_failed_blocking(&state.db, job.id, "missing/!uuid graph", 1);
        return;
    };
    // Read the LIVE graph so an edit / disable since the job was enqueued takes effect.
    match state::find_graph(&state.store, job.tenant_id, graph_id) {
        Some(g) if g.enabled => match &g.trigger {
            Trigger::Schedule(sched) => {
                fire_graph(state, job.tenant_id, graph_id, &g.concurrency, TriggerSource::Cron, HashMap::new());
                match next_occurrence(sched, Utc::now()) {
                    Some(next) => work_queue::reschedule_blocking(&state.db, job.id, &next.to_rfc3339()),
                    None => work_queue::mark_done_blocking(&state.db, job.id),
                }
            }
            // trigger changed away from Schedule → drop (reconcile owns re-arming)
            _ => work_queue::mark_done_blocking(&state.db, job.id),
        },
        // graph gone or disabled → drop; reconcile re-arms if it comes back enabled
        _ => work_queue::mark_done_blocking(&state.db, job.id),
    }
}
