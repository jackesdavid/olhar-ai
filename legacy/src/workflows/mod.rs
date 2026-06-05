//! Workflows — graph-based automation (spec 013).
//!
//! A WORKFLOWS screen holds multiple named graphs; each graph is an independent
//! automation: an engine-level trigger fires a DAG of typed, single-responsibility
//! nodes that runs once to completion. Mirrors the `src/map/` vertical slice.
//!
//! Built incrementally per `ai/specs/013-workflows-automation/tasks.md`:
//! Phase 1 — domain types, graph ops, persistence (this file's `domain` module).
#![allow(dead_code)] // types land phase-by-phase; wired into the app in Phase 6.

pub mod domain;
pub mod engine;
pub mod handler;
pub mod node;
pub mod view;

#[allow(unused_imports)] // consumed by later phases (editor / app wiring)
pub use domain::{
    ConcurrencyPolicy, Edge, Graph, GraphNode, NodeKind, Port, PortType, Trigger, WfMode,
    Workflows, WorkflowsState,
};
#[allow(unused_imports)]
pub use engine::{run_dag, Fire, WorkflowEngine};
