//! Workflows — graph-based automation (spec 013), ported for the web app.
//!
//! Holds multiple named graphs; each is an independent automation: an engine-level
//! trigger fires a DAG of typed, single-responsibility nodes that runs once to
//! completion. The ratatui editor (`handler`/`view`) is NOT ported — the Svelte
//! editor drives the same model via Tauri commands.
pub mod values;
pub mod domain;
pub mod engine;
pub mod node;
pub mod repo;

pub use domain::{
    ConcurrencyPolicy, Edge, EventSource, Graph, GraphNode, NodeKind, Port, PortType, RunStatus,
    Trigger, TriggerSource, VariableDecl, Viewport, Workflows, EXEC_IN, EXEC_OUT,
    WINDOW_STOP_MARKER,
};
pub use repo::{EventCategory, RunLogLine, RunRecord, TenantEvent};
pub use engine::{
    on_event_node, run_dag, run_dag_eventful, run_dag_with, run_subpath_eventful, Fire,
    WorkflowEngine,
};
pub use node::{literal_for, run_node, PortValue, PortValues};
