//! `Cooldown` — let one through, then ignore the rest for a while.
//!
//! The node between "a thing happened" and "tell somebody". Without it a sensor reporting forty
//! times a minute sends forty messages, and the person turns notifications off — which costs
//! more than the missed alert would have.
//!
//! The stamp is durable and instance-scoped, and both halves matter. Durable, because a cooldown
//! that resets when a pod restarts is not a cooldown — a crash loop becomes a message storm.
//! Instance-scoped, because two independent things running the same graph must not silence each
//! other.

use crate::workflows::host::SentinelHost;
use ggraph_core::host::{Host, Slot};
use ggraph_core::spec::{NodeError, NodeStep, Step, StepCx};
use ggraph_core::{PortName, Value};

pub struct Cooldown;

impl NodeStep<SentinelHost> for Cooldown {
    fn step(&self, cx: &mut StepCx<'_, SentinelHost>) -> Result<Step, NodeError> {
        let window_secs = cx
            .config
            .get("window_secs")
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .unwrap_or(60);

        // Stored in MILLISECONDS, under `last_ms`, because that is what the rows already in the
        // database say. The unit is not worth a migration and changing it silently would make
        // every live cooldown fire once more than it should.
        let key = cx.state_key(Slot::State);
        let now_ms = cx.host.now_epoch_secs().max(0) as u64 * 1000;
        let last_ms = cx
            .host
            .state()
            .get(&key)
            .and_then(|v| v.get("last_ms").and_then(serde_json::Value::as_u64));

        let elapsed_secs = match last_ms {
            // Never fired: always open, and the reported elapsed is past the window rather than
            // zero — zero would read downstream as "just fired", which is the opposite.
            None => window_secs + 1,
            Some(t) => now_ms.saturating_sub(t) / 1000,
        };
        let open = elapsed_secs >= window_secs;

        let mut out = ggraph_core::PortValues::new();
        out.insert(
            PortName::new("elapsed_secs"),
            Value::int(elapsed_secs as i64),
        );
        out.insert(PortName::new("ok"), Value::Bool(open));

        if !open {
            // No arm fires. The run continues; this branch simply ends here.
            return Ok(Step::outputs(out).logged(format!("{}s left", window_secs - elapsed_secs)));
        }

        cx.host
            .state()
            .set(&key, &serde_json::json!({ "last_ms": now_ms }));
        Ok(Step::outputs(out).arm("exec_out"))
    }
}
