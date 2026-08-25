//! Durable inbound-event consumer (card #57, unified onto `work_jobs` — card #64): claims
//! `workflow_event` jobs from the shared generic queue (`app_core::work_queue`) and fires each
//! into the engine as an `OnEvent` trigger. This replaces the lossy Redis `flow:inject`
//! subscriber — an event survives a flow restart and is never dropped when the fabric is down.
//!
//! Runs on a std thread; the queue's `*_blocking` wrappers bridge sqlx via `Db::block`, so no
//! tokio context is needed here (mirrors [`crate::engine`]'s poller and the notification consumer).
//! Polling is the durable claim; the queue's LISTEN/NOTIFY wakeup is a control-side latency win —
//! flow has no `PgPool`, so it stays on the poll.
use std::sync::Arc;
use std::time::Duration;

use app_core::work_queue;
use app_core::workflows::EventSource;

use crate::state::AppState;

/// Idle poll interval when the queue is empty.
const IDLE_POLL: Duration = Duration::from_millis(500);
/// Jobs claimed per pass.
const BATCH: i64 = 50;

pub fn spawn(state: Arc<AppState>) {
    std::thread::spawn(move || loop {
        let jobs = work_queue::claim_blocking(
            &state.db,
            work_queue::EVENT_CATEGORY,
            work_queue::EVENT_KIND,
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
    log::info!("inbound workflow_event consumer started (work_jobs)");
}

fn process(state: &Arc<AppState>, job: work_queue::Job) {
    // The payload is `{ event, source, fields }` — event/source are metadata, fields become the
    // OnEvent node's port values.
    let event = job
        .payload
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if event.is_empty() {
        log::warn!("workflow_event {}: missing event name; dropping", job.id);
        work_queue::mark_failed_blocking(&state.db, job.id, "missing event name", 1);
        return;
    }
    let source = job
        .payload
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("system")
        .to_string();
    let payload = payload_to_portvalues(job.payload.get("fields"));
    // Fire as a System event → fans out to this tenant's OnEvent(event) handler graphs.
    let matched = crate::engine::fire_event(
        state,
        job.tenant_id,
        EventSource::System {
            label: source.clone(),
        },
        event.clone(),
        None,
        payload,
    );
    // Zero listeners for a durable inbound event is a silent drop: the job is marked done but no
    // graph ran. Usually a STALE GRAPH STORE on this pod — a graph edit/reload only reached another
    // flow replica, so this pod still has the old (disabled/absent) copy. Surface it to Datadog
    // (service:flow) with the pod name so cross-pod divergence is queryable/alertable.
    if matched == 0 {
        // Distinguish the two very different reasons for zero listeners. `fire_event` filters on
        // `g.enabled` BEFORE matching, so a graph that handles this event but is switched off is
        // indistinguishable from one that doesn't exist — and the old message blamed a "stale graph
        // store" for both. A disabled workflow is ordinary user configuration, and warning about it
        // on every motion event buries the cross-pod divergence this log exists to catch.
        let (loaded, disabled_listeners) = {
            let store = state.store.lock().unwrap();
            let mine: Vec<_> = store
                .iter()
                .filter(|(t, _)| *t == job.tenant_id)
                .map(|(_, g)| g)
                .collect();
            let disabled = mine
                .iter()
                .filter(|g| !g.meta.enabled)
                .filter(|g| {
                    g.nodes.iter().any(|n| {
                        n.kind == app_core::workflows::domain::NodeKind::OnEvent
                            && n.config.get("event").and_then(|v| v.as_str())
                                == Some(event.as_str())
                    })
                })
                .count();
            (mine.len(), disabled)
        };
        if disabled_listeners > 0 {
            log::info!(
                "inbound event '{event}' (source '{source}', tenant {tenant}) matched 0 graphs: \
                 {disabled_listeners} graph(s) handle it but are disabled — enable one to run it; \
                 job {job} consumed with no run",
                tenant = job.tenant_id,
                job = job.id,
            );
        } else {
            log::warn!(
                "inbound event '{event}' (source '{source}', tenant {tenant}) matched 0 graphs on \
                 pod {pod} and NO graph here handles it — possible stale graph store (this pod \
                 holds {loaded} graph(s) for the tenant); job {job} consumed with no run",
                tenant = job.tenant_id,
                pod = state.pod_name,
                job = job.id,
            );
        }
    }
    work_queue::mark_done_blocking(&state.db, job.id);
}

/// Map an inbound JSON payload object to typed `PortValues` (string→Text or Image,
/// integer/float→Num, bool→Bool). Strings that decode as a JPEG (FFD8FF magic) become
/// `Image` — this lets the edge embed the triggering frame inline without a second round-trip.
fn payload_to_portvalues(payload: Option<&serde_json::Value>) -> app_core::workflows::PortValues {
    use app_core::workflows::PortValue;
    use base64::Engine as _;
    let mut pv = app_core::workflows::PortValues::new();
    if let Some(obj) = payload.and_then(|p| p.as_object()) {
        for (k, v) in obj {
            let val = if let Some(arr) = v.as_array() {
                // NumArray ports (e.g. motion `bbox` = [x,y,w,h]) → PortValue::List.
                PortValue::List(arr.iter().map(json_num_to_pv).collect())
            } else if let Some(s) = v.as_str() {
                // Attempt base64 decode; if the result starts with JPEG magic it's an image.
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
                    if bytes.len() > 3
                        && bytes[0] == 0xFF_u8
                        && bytes[1] == 0xD8
                        && bytes[2] == 0xFF
                    {
                        PortValue::Image(bytes)
                    } else {
                        str_to_pv(s)
                    }
                } else {
                    str_to_pv(s)
                }
            } else if let Some(n) = v.as_i64() {
                PortValue::Num(n)
            } else if let Some(f) = v.as_f64() {
                PortValue::Num(f.round() as i64)
            } else if let Some(b) = v.as_bool() {
                PortValue::Bool(b)
            } else {
                continue;
            };
            pv.insert(k.clone(), val);
        }
    }
    pv
}

/// A single JSON number → PortValue::Num (floats rounded). Non-numbers → Text.
fn json_num_to_pv(v: &serde_json::Value) -> app_core::workflows::PortValue {
    use app_core::workflows::PortValue;
    if let Some(n) = v.as_i64() {
        PortValue::Num(n)
    } else if let Some(f) = v.as_f64() {
        PortValue::Num(f.round() as i64)
    } else if let Some(s) = v.as_str() {
        PortValue::Text(s.to_string())
    } else {
        PortValue::Text(v.to_string())
    }
}

/// A scalar string payload value. Defensively decodes a JSON-array or comma-joined
/// number string (e.g. a legacy `bbox` "x,y,w,h") into a NumArray List; otherwise Text.
fn str_to_pv(s: &str) -> app_core::workflows::PortValue {
    use app_core::workflows::PortValue;
    let t = s.trim();
    if t.starts_with('[') {
        if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(t) {
            return PortValue::List(arr.iter().map(json_num_to_pv).collect());
        }
    }
    // Comma-joined integers like "12,34,56,78" → List; anything else stays Text.
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    if parts.len() >= 2 && parts.iter().all(|p| p.parse::<i64>().is_ok()) {
        return PortValue::List(
            parts
                .iter()
                .map(|p| PortValue::Num(p.parse().unwrap()))
                .collect(),
        );
    }
    PortValue::Text(s.to_string())
}
