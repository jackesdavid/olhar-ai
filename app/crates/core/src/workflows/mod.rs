//! Workflows — graph-based automation (spec 013), ported for the web app.
//!
//! Holds multiple named graphs; each is an independent automation: an engine-level
//! trigger fires a DAG of typed, single-responsibility nodes that runs once to
//! completion. The ratatui editor (`handler`/`view`) is NOT ported — the Svelte
//! editor drives the same model via Tauri commands.
pub mod domain;
pub mod engine;
pub mod node;

pub use domain::{
    load_workflows, save_workflows, ConcurrencyPolicy, Edge, Graph, GraphNode, NodeKind, Port,
    PortType, Trigger, Viewport, Workflows,
};
pub use engine::{run_dag, run_dag_with, Fire, WorkflowEngine};
pub use node::{literal_for, run_node, PortValue, PortValues};
