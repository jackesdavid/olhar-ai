//! The "chat" feature: a text conversation with BISHOP/AP.
//!
//! `domain` holds the conversation log and Bishop's canned lines; `handler` is
//! the offline persona that answers an inquiry. To make this a real assistant,
//! swap [`handler::reply`] for a call out to an LLM (e.g. the Anthropic API) —
//! the conversation plumbing and views stay identical.
pub mod domain;
pub mod handler;
pub mod view;

pub use domain::{random_greeting, Conversation};
