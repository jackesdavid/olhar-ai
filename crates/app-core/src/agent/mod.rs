//! The live assistant: Bishop backed by the Anthropic (Claude) API, copied from
//! the ratatui `src/agent` for the web app.
//!
//! `domain` holds the conversation message type, the API configuration, and the
//! dynamic personality. `client` is the outbound tool-use loop to `/v1/messages`.
//! `vision` is the one-shot image describe (Claude). `detector` is native YOLOX-Nano
//! person/object detection via `tract` — fully local, no sidecar. `face` embeds faces
//! for enrollment/recognition via the GPU `ai_service` sidecar. `ai_service` is the
//! op-dispatched client for that portable Python vision service (RunPod/local).
//! `tools` is the capability registry — adapted so its UI-effect channel carries
//! **web-friendly** events the Tauri shell re-emits (instead of the ratatui
//! modal/`GrayImage` effects).
//!
//! Rust has no official Anthropic SDK, so this uses raw HTTP (ureq) — the
//! supported approach for unsupported-SDK languages.
pub mod ai_service;
pub mod client;
pub mod detector;
pub mod domain;
pub mod face;
pub mod face_detector;
pub mod face_quality;
pub mod tools;
pub mod vision;

pub use domain::{AgentConfig, AgentOutcome, Message, Role};
pub use tools::{ToolContext, ToolRegistry, ToolUiEvent};
