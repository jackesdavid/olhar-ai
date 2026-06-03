//! The "map" feature: a configurable sci-fi floor plan. The operator lays out
//! rooms on a coordinate grid and assigns camera devices to each, so the system
//! knows which camera covers which room. Persisted to `map.json`.
//!
//! `domain` holds [`MapRoom`] + the grid model + `map.json` load/save and the
//! transient [`MapState`]. `handler` is the editing logic (selection, geometry
//! edits, rename, camera assignment). `view` renders the floor-plan canvas and
//! the context panel.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{MapMode, MapState};
