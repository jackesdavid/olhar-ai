//! `DelayTrigger` — one signal when a burst starts, one when it goes quiet.
//!
//! Something starts happening and keeps happening: a door held open, a person in frame. What a
//! graph usually wants is not one message per signal but *"it started"* and, once things settle,
//! *"it stopped"*.
//!
//! The trailing edge is the hard half, because nothing happens when a burst ends — the absence
//! of a signal is not a signal. So the window is a ROW, not a thread: `try_arm` opens it,
//! a `work_jobs` entry with `run_after` at the deadline closes it, and the closing arrives as a
//! new run entered at this node. It survives a pod restart for the same reason.
//!
//! ## The variables, which are the subtle part
//!
//! Variables are run-scoped, and the trailing edge is a DIFFERENT run — so a `Get` below the
//! stop arm read a type-zero for whatever a `Set` wrote on the arming run (#114). The fix is a
//! snapshot: the whole variable map is written to this node's own row whenever it arms or
//! extends, and restored here before `stop` fires. Both halves used to live in `run_driver` as
//! DelayTrigger-shaped special cases; they belong to the node, and the engine has a state slot
//! named for exactly this.
//!
//! ## The trap
//!
//! The test for "the window closed" is the MARKER in the entry payload, not `forced` alone. A
//! loop back-edge forces a node too, and mistaking one for the other makes a loop close a window
//! that is still open.

use crate::workflows::host::SentinelHost;
use crate::workflows::values;
use ggraph_core::host::{Host, Slot};
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};

pub struct DelayTrigger;

fn secs(cfg: &serde_json::Value, key: &str, fallback: i64) -> i64 {
    cfg.get(key)
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .unwrap_or(fallback)
}

impl NodeStep<SentinelHost> for DelayTrigger {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let wait_secs = secs(cx.config, "wait_secs", 10);
        let extend_secs = secs(cx.config, "extend_secs", wait_secs);
        let now = cx.host.now_epoch_secs();
        let state_key = cx.state_key(Slot::State);
        let vars_key = cx.state_key(Slot::Variables);

        // Trailing edge. The queue consumer already won the disarm, so this run owns the stop.
        if cx.forced
            && cx
                .entry_payload
                .contains_key(crate::workflows::domain::WINDOW_STOP_MARKER)
        {
            // Put the arming run's variables back before anything downstream reads them.
            if let Some(stored) = cx.host.state().get(&vars_key) {
                let restored = values::variables_from_json(&stored);
                let mut map = cx.vars.lock().unwrap();
                for (k, v) in restored {
                    map.insert(
                        ggraph_core::PortName::new(k),
                        crate::workflows::compat::to_engine(&v),
                    );
                }
            }
            return Ok(Step::default().arm("stop").logged("window closed"));
        }

        let expires_at = crate::timeutil::plus_secs_rfc3339(wait_secs);

        if cx.host.state().try_arm(&state_key, now + wait_secs) {
            // Leading edge: this signal opened the window, so it owns the start.
            cx.host.schedule(now + wait_secs, cx.target()).ok();
            snapshot_variables(cx, &vars_key);
            return Ok(Step::default()
                .arm("start")
                .logged(format!("armed for {wait_secs}s → start")));
        }

        // Already open. A re-trigger tops the window up, and only when that actually pushes the
        // deadline out — `remaining = max(remaining, extend)`, which is Time Gate's rule and the
        // reason a steady trickle of signals does not extend a window forever.
        let expires_ts = cx
            .host
            .state()
            .get(&state_key)
            .and_then(|v| {
                v.get("expires_at")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|t| t.timestamp())
            .unwrap_or(now);
        let remaining = expires_ts - now;
        snapshot_variables(cx, &vars_key);

        if remaining < extend_secs {
            cx.host.state().extend(&state_key, now + extend_secs);
            // The scheduled stop still carries the old expiry: the consumer re-reads the state
            // when it comes due and reschedules itself if the window moved.
            return Ok(Step::default().logged(format!("extended +{}s", extend_secs - remaining)));
        }
        let _ = expires_at;
        Ok(Step::default().logged(format!("re-trigger, {remaining}s remaining (no extend)")))
    }
}

/// The run's variables, onto this node's own row.
///
/// Written whenever the node arms or extends, so the row always holds the most recent arming
/// run's view — and gated to this node's instance, so an unrelated run of the same graph cannot
/// overwrite a live window's snapshot.
fn snapshot_variables(cx: &StepCx<'_, SentinelHost>, key: &ggraph_core::StateKey) {
    let legacy: std::collections::HashMap<String, crate::workflows::node::PortValue> = cx
        .vars
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(k, v)| {
            crate::workflows::compat::from_engine(v).map(|v| (k.as_str().to_string(), v))
        })
        .collect();
    cx.host
        .state()
        .set(key, &values::variables_to_json(&legacy));
}
