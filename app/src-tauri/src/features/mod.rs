//! Per-feature Tauri adapters. Each submodule owns one feature slice's
//! `#[tauri::command]` handlers, background pollers, and managed-state lifecycle —
//! the Tauri-specific glue that can't live in the UI-agnostic `app_core`. `lib.rs`
//! stays a thin assembler that wires these together.
pub mod alerts;
pub mod chat;
pub mod devices;
pub mod map;
pub mod notifications;
pub mod people;
pub mod settings;
pub mod stream;
pub mod voice;
pub mod workflows;
