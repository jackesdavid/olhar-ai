//! Firing a cross-graph event from inside a run.
//!
//! `EmitEvent` is ten lines in the scheduler and was the last of the cheap conversions — except
//! it is not a conversion at all. Its callback reaches `fire_event`, which needs the always-on
//! runtime's own state: the listener registry, the tenant's devices for camera-scoped delivery,
//! the coalescing window. None of that is in a [`ToolContext`], and none of it should be in an
//! engine trait either — "an event" here means a tenant-scoped, camera-gated, listener-fanned
//! thing that a graph can be woken by, which is this product's idea and nobody else's.
//!
//! So it goes where `ui_tx` and `notifier` already are: on this product's own context, supplied
//! by whoever is running the graph. A host that has no event bus leaves it `None` and the node
//! says so rather than pretending it fired.

use super::node::PortValues;
use uuid::Uuid;

/// Fires a named event, to be delivered to whatever is listening for it.
pub trait EventEmitter: Send + Sync {
    /// `graph` is the emitter's own id — the implementation resolves its name, because the name
    /// is for display and the id is what identifies it.
    fn emit(&self, graph: Uuid, event: &str, run_id: Uuid, payload: &PortValues);
}
