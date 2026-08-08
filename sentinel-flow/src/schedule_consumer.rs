//! Durable calendar-schedule consumer (#111, per-node since the cron selector). Each cron-mode
//! `When Event` node in an enabled graph has ONE
//! pending `work_jobs` row whose `run_after` is its next fire (computed in the graph's timezone).
//! The consumer claims due rows → fires the graph (as a `Cron` source) → reschedules the same row to
//! the following occurrence. [`reconcile`] rebuilds the rows from the current enabled graphs — called
//! on boot and on every graph reload — so schedule edits / enable / disable take effect at once.
//!
//! Option B (queue) from the plan: calendar math stays out of the 1s poller, and an armed schedule
//! survives a pod restart (the next fire is a row in Postgres, not a held thread).
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use app_core::work_queue;
use app_core::workflows::domain::TriggerSource;
use app_core::workflows::schedule::next_occurrence;

use crate::engine::fire_graph;
use crate::state::{self, AppState};

pub const SCHEDULE_CATEGORY: &str = "workflow_schedule";
pub const SCHEDULE_KIND: &str = "fire";

const IDLE_POLL: Duration = Duration::from_millis(500);
const BATCH: i64 = 50;

/// Rebuild the pending schedule jobs from every enabled graph's CRON NODES. Deletes the pending
/// ones and re-enqueues one per node at its next occurrence. Idempotent (`next_occurrence` is
/// deterministic for a given schedule + now), so it is safe to call on every reload.
///
/// Per NODE, not per graph: a graph may hold several `When Event` nodes in cron mode, which is the
/// whole point — one turning devices on and another turning them off, from one graph.
pub fn reconcile(state: &AppState) {
    work_queue::delete_pending_by_category_blocking(&state.db, SCHEDULE_CATEGORY);
    let now = Utc::now();
    let jobs: Vec<(Uuid, Uuid, Option<u32>, String)> = {
        let s = state.store.lock().unwrap();
        s.iter()
            .filter(|(_, g)| g.enabled)
            // One job per SCHEDULED NODE, not per graph. A graph may hold several `When Event`
            // nodes in cron mode — the driving case is one turning devices on and another turning
            // them off — and the graph-keyed model could not express that at all.
            .flat_map(|(t, g)| {
                g.nodes
                    .iter()
                    .filter_map(|n| {
                        let c = n.cron()?;
                        next_occurrence(&c.schedule, now)
                            .map(|next| (*t, g.id, Some(n.id), next.to_rfc3339()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let n = jobs.len();
    for (tenant, graph_id, node_id, run_after) in jobs {
        // The node id is what makes the fire ADDRESSED. Without it two cron nodes in one graph
        // would both wake on either tick, running the whole graph twice and toggling a device at
        // random — the failure the two-device case shows immediately.
        let payload = json!({
            "graph": graph_id.to_string(),
            "node": node_id,
            "due_at": run_after,
        });
        work_queue::enqueue_at_blocking(
            &state.db,
            tenant,
            SCHEDULE_CATEGORY,
            SCHEDULE_KIND,
            payload,
            &run_after,
        );
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
    let Some(graph_id) = job
        .payload
        .get("graph")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        work_queue::mark_failed_blocking(&state.db, job.id, "missing/!uuid graph", 1);
        return;
    };
    let node_id = job.payload.get("node").and_then(|v| v.as_u64()).map(|n| n as u32);
    // Read the LIVE graph so an edit / disable since the job was enqueued takes effect.
    let Some(g) = state::find_graph(&state.store, job.tenant_id, graph_id).filter(|g| g.enabled)
    else {
        // graph gone or disabled → drop; reconcile re-arms if it comes back enabled
        work_queue::mark_done_blocking(&state.db, job.id);
        return;
    };
    let Some(cron) = node_id.and_then(|nid| g.nodes.iter().find(|n| n.id == nid)?.cron()) else {
        // the node was deleted or is no longer a cron — reconcile owns re-arming
        work_queue::mark_done_blocking(&state.db, job.id);
        return;
    };

    // A late fire runs only inside its grace window.
    //
    // The job waits in `work_jobs` with a past `run_after` while flow is down, so on return it is
    // claimed however late — which used to mean a bedtime switch could fire at breakfast. `due_at`
    // is the time this fire was FOR, so lateness is measurable rather than assumed.
    let late = job
        .payload
        .get("due_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|due| (Utc::now() - due.with_timezone(&Utc)).num_seconds().max(0) as u64)
        .unwrap_or(0);
    if late > cron.grace_secs {
        log::warn!(
            "cron[{graph_id}/{:?}]: skipped — {late}s late, past its {}s grace window",
            node_id,
            cron.grace_secs
        );
    } else {
        crate::engine::spawn_node_subrun(
            state,
            job.tenant_id,
            graph_id,
            node_id.unwrap_or(0),
            TriggerSource::Cron,
            app_core::workflows::PortValues::new(),
        );
    }

    match next_occurrence(&cron.schedule, Utc::now()) {
        Some(next) => work_queue::reschedule_blocking(&state.db, job.id, &next.to_rfc3339()),
        None => work_queue::mark_done_blocking(&state.db, job.id),
    }
}
