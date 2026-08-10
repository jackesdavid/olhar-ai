//! Device Gate resume sweep (card #176): delivers every finished request to its graph.
//!
//! Two jobs that are really one — "this request is finished and the graph has not been told":
//!
//!  * A TTL passed with nobody deciding → the node resumes down `failure`. Without this a phone
//!    left face-down would leave the graph silently unfinished, which is worse than a denial
//!    because nothing in the run history would say so.
//!  * A person answered and the delivery has not happened yet. `sentinel-control` settles the row
//!    (it holds the session, so it does the authorizing) and nudges flow over HTTP — but that
//!    nudge reaches ONE replica behind a load-balanced DNS name and can simply be lost. The sweep
//!    is what makes the answer durable; the nudge only makes it fast.
//!
//! **Not on `work_jobs`, unlike the timing-window consumer.** The request row is already the thing
//! two pods must not both act on, and it already holds the deadline. A parallel job would be a
//! second place the same fact lives, and the two would disagree the moment an answer settles a
//! request early. One source of truth, polled — and being the source of truth is exactly what lets
//! it absorb the lost-nudge case above.
//!
//! Runs on a std thread; `Db::block` bridges sqlx, so no tokio context is needed (mirrors
//! [`crate::window_consumer`]).
use std::sync::Arc;
use std::time::Duration;

use app_core::workflows::device_gate::{self, GateState};
use app_core::workflows::domain::{DEVICE_GATE_FACTOR, DEVICE_GATE_VERDICT, TriggerSource};
use app_core::workflows::{PortValue, PortValues};

use crate::state::AppState;

/// How often to look for a lapsed request. A gate TTL is minutes at the shortest (decision 05: a
/// push is best-effort, so the deadline assumes the phone is CHECKED, not that the person was
/// alerted), which makes five seconds of lateness on the `failure` arm irrelevant — and it keeps
/// the sweep off the database in the overwhelmingly common case of nothing pending.
const IDLE_POLL: Duration = Duration::from_secs(5);
/// Cap the requests settled per pass so one backlog cannot monopolise the thread.
const BATCH: usize = 25;

pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || loop {
        let mut settled = 0;
        // `claim_one_resumable` claims ONE row atomically (`FOR UPDATE SKIP LOCKED`), so two flow
        // pods sweeping at the same moment take different rows and neither resumes a request the
        // other already did.
        while settled < BATCH {
            let Some(target) = device_gate::claim_one_resumable(&state.db) else {
                break;
            };
            resume(&state, target);
            settled += 1;
        }
        if settled == 0 {
            std::thread::sleep(IDLE_POLL);
        }
    });
    log::info!("device gate resume sweep started");
}

/// Resume the gate's node, carrying the verdict so the node knows which arm to fire.
pub fn resume(state: &Arc<AppState>, target: device_gate::ResumeTarget) {
    log::info!(
        "device gate {}: tenant {} graph {} node {} → {}",
        target.state.as_str(),
        target.tenant_id,
        target.graph_id,
        target.node_id,
        target.state.exec_arm()
    );
    crate::engine::spawn_node_subrun(
        state,
        target.tenant_id,
        target.graph_id,
        target.node_id,
        TriggerSource::Timer,
        verdict_payload(&target),
    );
}

/// The entry payload that tells a Device Gate node what was decided. Its PRESENCE is what marks
/// this as an answer-resume rather than an ordinary force — a loop back-edge forces a node too,
/// and firing an arm nobody chose would be worse than not resuming.
pub fn verdict_payload(target: &device_gate::ResumeTarget) -> PortValues {
    let mut pv = PortValues::new();
    pv.insert(
        DEVICE_GATE_VERDICT.to_string(),
        PortValue::Text(target.state.as_str().to_string()),
    );
    pv.insert(
        app_core::workflows::domain::DEVICE_GATE_ANSWERED_BY.to_string(),
        PortValue::Text(target.answered_by_label.clone()),
    );
    pv.insert(
        DEVICE_GATE_FACTOR.to_string(),
        PortValue::Text(
            target
                .auth_factor
                .map(|f| f.as_str().to_string())
                .unwrap_or_default(),
        ),
    );
    debug_assert!(
        target.state != GateState::Pending,
        "a pending request must never be resumed — it has no verdict to carry"
    );
    pv
}
