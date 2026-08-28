//! Workflows — graph-based automation (spec 013), ported for the web app.
//!
//! Holds multiple named graphs; each is an independent automation: an engine-level
//! trigger fires a DAG of typed, single-responsibility nodes that runs once to
//! completion. The ratatui editor (`handler`/`view`) is NOT ported — the Svelte
//! editor drives the same model via Tauri commands.
pub mod behaviour;
pub mod catalog;
pub mod compat;
pub mod device_gate;
pub mod domain;
pub mod emit;
pub mod gate;
#[cfg(test)]
mod golden;
pub mod host;
pub mod node;
pub mod observer;
pub mod readiness;
pub mod registry;
pub mod repo;
pub mod run;
pub mod schedule;
pub mod steps;
pub mod values;

pub use catalog::catalog_json;
pub use domain::{
    ConcurrencyPolicy, Edge, EventSource, Graph, GraphNode, InstanceScope, NodeKind, Port,
    PortType, RunStatus, Schedule, ScheduleFreq, Trigger, TriggerSource, VariableDecl, Viewport,
    Workflows, EXEC_IN, EXEC_OUT, WINDOW_STOP_MARKER,
};
pub use gate::{Fire, WorkflowEngine};
pub use host::{SentinelHost, SentinelMeta};
pub use node::{literal_for, run_node, PortValue, PortValues};
pub use steps::on_event::on_event_node;
// The facade's promise: a caller reports what a run did without naming the engine that ran it.
pub use ggraph_core::host::{Observer, UiEvent};
pub use repo::{EventCategory, RunLogLine, RunRecord, TenantEvent};
pub use run::{run as run_graph, Start};
