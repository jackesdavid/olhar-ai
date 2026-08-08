//! Durable timing-window consumer (card #86): claims due `workflow_window` jobs from the shared
//! queue (`app_core::work_queue`) and delivers a `DelayTrigger`'s trailing edge — a run forced at
//! that node, which fires its `stop`.
//!
//! Why a queue and not a held thread: a node's handler runs to completion before the engine
//! settles it, so a `DelayTrigger` that blocked would also hold back its own `start` branch. The
//! wait is a row in Postgres instead, which means an armed window survives a pod restart — the
//! stop fires late rather than never.
//!
//! Runs on a std thread; the queue's `*_blocking` wrappers bridge sqlx via `Db::block`, so no
//! tokio context is needed (mirrors [`crate::event_consumer`]).
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use app_core::work_queue;
use app_core::workflows::{self, PortValue, PortValues};

use crate::state::AppState;

/// Idle poll interval when nothing is due. Also the worst-case lateness of a `stop`, which is
/// irrelevant next to a debounce window measured in seconds.
const IDLE_POLL: Duration = Duration::from_millis(500);
/// Jobs claimed per pass.
const BATCH: i64 = 50;

pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || loop {
        let jobs = work_queue::claim_blocking(
            &state.db,
            work_queue::WINDOW_CATEGORY,
            work_queue::WINDOW_KIND,
            BATCH,
        );
        if jobs.is_empty() {
            std::thread::sleep(IDLE_POLL);
            continue;
        }
        for job in jobs {
            process(&state, job);
        }
    });
    log::info!("workflow timing-window consumer started (work_jobs)");
}

fn process(state: &Arc<AppState>, job: work_queue::Job) {
    // Payload is `{ graph, node, expires_at }` — self-contained, so the timer needs nothing else.
    let Some(graph_id) = job
        .payload
        .get("graph")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        log::warn!("workflow_window {}: missing/!uuid graph; dropping", job.id);
        work_queue::mark_failed_blocking(&state.db, job.id, "missing graph id", 1);
        return;
    };
    let Some(node_id) = job.payload.get("node").and_then(|v| v.as_u64()) else {
        log::warn!("workflow_window {}: missing node id; dropping", job.id);
        work_queue::mark_failed_blocking(&state.db, job.id, "missing node id", 1);
        return;
    };
    let node_id = node_id as u32;
    // Which instance's window this closes. Absent on jobs enqueued before instances existed —
    // those are Single-scope graphs, whose key is "" anyway, so the default is also the migration.
    let instance_key = job
        .payload
        .get("instance")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // Close the window — but only if it's still armed AND still expired. This single conditional
    // write is what makes the cycle exactly-once: a re-trigger that extended the window loses
    // nothing (we simply don't win), and two consumers can't both fire the stop.
    let won = workflows::repo::try_disarm_expired_node_state(
        &state.db,
        job.tenant_id,
        graph_id,
        node_id,
        &instance_key,
    );
    if !won {
        // Either the window moved (extended past now) or it's already closed. Re-read the state:
        // still armed → follow it to its new expiry; otherwise the cycle is over and so is this job.
        let state_json = workflows::repo::get_node_state(
            &state.db,
            job.tenant_id,
            graph_id,
            node_id,
            &instance_key,
        );
        let expires_at = state_json
            .as_ref()
            .filter(|v| v.get("state").and_then(|s| s.as_str()) == Some("armed"))
            .and_then(|v| v.get("expires_at"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        match expires_at {
            Some(next) => work_queue::reschedule_blocking(&state.db, job.id, &next),
            None => work_queue::mark_done_blocking(&state.db, job.id),
        }
        return;
    }

    // We closed it, so this job owns the trailing edge.
    crate::engine::spawn_node_subrun(
        state,
        job.tenant_id,
        graph_id,
        node_id,
        app_core::workflows::domain::TriggerSource::Timer,
        stop_payload(),
    );
    work_queue::mark_done_blocking(&state.db, job.id);
}

/// The entry payload that marks a run as a window expiry, so the node can tell a trailing-edge
/// delivery from an ordinary re-trigger (a loop back-edge can force a node too).
pub fn stop_payload() -> PortValues {
    let mut pv = PortValues::new();
    pv.insert(
        app_core::workflows::WINDOW_STOP_MARKER.to_string(),
        PortValue::Bool(true),
    );
    pv
}
