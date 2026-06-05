//! The live assistant: Bishop backed by the Anthropic (Claude) API.
//!
//! `domain` holds the conversation message type, the API configuration, and the
//! dynamic personality loaded from `assets/personality.md`. `client` is the
//! outbound HTTP call to `/v1/messages`. Mirrors the `voice` slice: domain +
//! client, talking to an external service.
//!
//! Rust has no official Anthropic SDK, so this uses raw HTTP (ureq) — the
//! supported approach for unsupported-SDK languages.
pub mod client;
pub mod domain;
pub mod tools;
pub mod vision;

pub use domain::{AgentOutcome, Message};
