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

    // Read the held value BEFORE disarming — the disarm overwrites state_json, and a read-back
    // after it would return the post-update document (the `UPDATE … RETURNING` trap, one step
    // removed). The order is safe: the row is `armed` until we disarm it, so nobody can re-arm
    // underneath us, and an extend racing us only changes `expires_at` — which makes our disarm
    // lose, so we don't fire at all.
    let carry = workflows::repo::get_node_state(&state.db, job.tenant_id, graph_id, node_id)
        .and_then(|v| v.get("carry").cloned())
        .filter(|v| !v.is_null());

    // Close the window — but only if it's still armed AND still expired. This single conditional
    // write is what makes the cycle exactly-once: a re-trigger that extended the window loses
    // nothing (we simply don't win), and two consumers can't both fire the stop.
    let won =
        workflows::repo::try_disarm_expired_node_state(&state.db, job.tenant_id, graph_id, node_id);
    if !won {
        // Either the window moved (extended past now) or it's already closed. Re-read the state:
        // still armed → follow it to its new expiry; otherwise the cycle is over and so is this job.
        let state_json =
            workflows::repo::get_node_state(&state.db, job.tenant_id, graph_id, node_id);
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
    crate::engine::spawn_node_subrun(state, job.tenant_id, graph_id, node_id, carry);
    work_queue::mark_done_blocking(&state.db, job.id);
}

/// The entry payload for a window expiry: the marker that tells the node this is a trailing-edge
/// delivery rather than an ordinary re-trigger (a loop back-edge can force a node too), plus the
/// camera the window was armed with.
///
/// The camera has to travel with the run. This run came from a timer, so it has none of the
/// event's data — and `StopRecording.camera` is required. Without it the stop branch resolves to
/// nothing and is skipped as a dead path: a stop that fires and stops nothing.
pub fn stop_payload(carry: Option<serde_json::Value>) -> PortValues {
    let mut pv = PortValues::new();
    pv.insert(
        app_core::workflows::WINDOW_STOP_MARKER.to_string(),
        PortValue::Bool(true),
    );
    if let Some(cam) = carry.as_ref().and_then(camera_from_json) {
        pv.insert(
            app_core::workflows::WINDOW_CARRY_KEY.to_string(),
            PortValue::Camera(cam),
        );
    }
    pv
}

/// Rebuild the wire-subset camera the leading edge stored (`{id, ip, name}`).
fn camera_from_json(v: &serde_json::Value) -> Option<app_core::workflows::domain::CameraEntity> {
    let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let (id, ip, name) = (field("id"), field("ip"), field("name"));
    // An entity with no ip is useless to StopRecording, which stops by camera_ip.
    if ip.is_empty() {
        return None;
    }
    Some(app_core::workflows::domain::CameraEntity { id, ip, name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::workflows::WINDOW_STOP_MARKER;

    #[test]
    fn stop_payload_always_marks_the_window() {
        // The marker is what separates a trailing edge from a loop back-edge forcing the node.
        // It must be there whether or not a camera was carried.
        assert!(stop_payload(None).contains_key(WINDOW_STOP_MARKER));
        assert!(!stop_payload(None).contains_key(app_core::workflows::WINDOW_CARRY_KEY));
    }

    #[test]
    fn stop_payload_carries_the_armed_camera() {
        let carry = serde_json::json!({ "id": "uuid:1", "ip": "192.168.1.184", "name": "Hall" });
        let pv = stop_payload(Some(carry));
        match pv.get(app_core::workflows::WINDOW_CARRY_KEY) {
            Some(PortValue::Camera(c)) => {
                assert_eq!(c.ip, "192.168.1.184");
                assert_eq!(c.name, "Hall");
            }
            other => panic!("expected a Camera on the carry key, got {other:?}"),
        }
    }

    #[test]
    fn a_camera_without_an_ip_is_not_carried() {
        // StopRecording stops by camera_ip. An entity without one would sail through the engine
        // as a resolved input and then fail at the edge — better to carry nothing and have the
        // branch die visibly than to hand on a camera that can't be stopped.
        assert!(camera_from_json(&serde_json::json!({ "id": "x", "name": "Hall" })).is_none());
        assert!(camera_from_json(&serde_json::json!({ "ip": "" })).is_none());
    }

    #[test]
    fn a_null_carry_is_not_a_camera() {
        // A DelayTrigger used purely for timing stores carry: null — that must not become an
        // empty camera that satisfies StopRecording's required port.
        assert!(camera_from_json(&serde_json::Value::Null).is_none());
    }
}
