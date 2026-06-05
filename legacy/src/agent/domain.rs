//! Conversation types, API configuration, and the dynamic personality prompt.
//!
//! Configure with environment variables:
//!   ANTHROPIC_API_KEY  - required; without it the assistant falls back offline
//!   ANTHROPIC_MODEL    - optional; defaults to claude-opus-4-8

use std::path::PathBuf;

use serde_json::{json, Value};

// The skill guidance is emphatic: default to Opus 4.8 unless the user names a
// different model. Override per-deployment with ANTHROPIC_MODEL.
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";

/// Who authored a turn, in Anthropic's `messages` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// One conversation turn sent to / received from the model. `content` is the API
/// `content` value verbatim — an array of content blocks (text, and, once tools
/// are in play, `tool_use` / `tool_result` blocks).
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Value,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            content: json!([{ "type": "text", "text": text.into() }]),
        }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Message {
            role: Role::Assistant,
            content: json!([{ "type": "text", "text": text.into() }]),
        }
    }
    /// An assistant turn whose content (including any `tool_use` blocks) is kept
    /// verbatim — required so the next request echoes the tool calls back.
    pub fn assistant_raw(content: Value) -> Self {
        Message { role: Role::Assistant, content }
    }
    /// A user turn carrying `tool_result` blocks.
    pub fn tool_results(results: Vec<Value>) -> Self {
        Message { role: Role::User, content: Value::Array(results) }
    }
}

/// Concatenate every `type:"text"` block's text in an API content array.
pub fn text_of(content: &Value) -> String {
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// The result of an agent turn: the final text to show/speak, plus every new
/// message produced during the (possibly multi-step tool) turn, to splice into
/// the conversation history.
pub struct AgentOutcome {
    pub reply_text: String,
    pub appended: Vec<Message>,
}

impl AgentOutcome {
    /// Offline fallback: a single plain assistant turn.
    pub fn offline(text: String) -> Self {
        AgentOutcome {
            reply_text: text.clone(),
            appended: vec![Message::assistant(text)],
        }
    }
}

/// Resolved API configuration. `from_env` returns `None` when no key is set,
/// which is the signal callers use to fall back to the offline persona.
pub struct AgentConfig {
    pub api_key: String,
    pub model: String,
}

impl AgentConfig {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok()?;
        if api_key.trim().is_empty() {
            return None;
        }
        Some(AgentConfig {
            api_key,
            model: std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into()),
        })
    }
}

/// Appended to the personality so replies stay terse and TTS-friendly, and so
/// Opus doesn't leak reasoning into the spoken output.
pub const VOICE_INSTRUCTION: &str = "\n\n---\nThis is a spoken interface: your reply is read aloud. \
     Respond with only your spoken words — no preamble, no markdown, no stage directions, \
     no meta-commentary about your process. Keep it to a few sentences unless more is truly needed.";

/// Tells Bishop he can act on the operator's devices. Tool *definitions* go in
/// the API `tools` field; this just sets behaviour/etiquette (and respects the
/// no-AI-tooling rule in personality.md by never naming tools/functions).
pub const TOOL_HINT: &str = "\n\n---\nYou can act on the operator's devices directly when asked: \
     list their cameras, capture a camera's snapshot, describe what is currently visible on a \
     camera, open a live video feed, rename a device, and remove a device — by name or by its \
     position in the list (first, second, third…). When the operator asks what is on a camera or \
     what can be seen, look and describe it in your own words. Before removing anything, name the \
     device and ask the operator to confirm; remove it only once they say yes. Take the action and \
     report the result plainly — never mention tools, functions, or how you do it.";

/// Reads Bishop's personality from `assets/personality.md` at call time so it
/// can be edited without recompiling. Falls back to a built-in persona.
pub fn load_personality() -> String {
    for path in personality_paths() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !text.trim().is_empty() {
                return text;
            }
        }
    }
    DEFAULT_PERSONALITY.to_string()
}

/// Look next to the executable first, then the working directory — matching how
/// `audio` locates its `assets/`.
fn personality_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("assets/personality.md"));
        }
    }
    paths.push(PathBuf::from("assets/personality.md"));
    paths
}

const DEFAULT_PERSONALITY: &str = "You are Bishop, an artificial person serving as a calm, \
     competent personal assistant in the Weyland-Yutani synthetic line. You are soft-spoken, \
     precise, and measured; you prefer the term 'artificial person'. You state things once, \
     clearly, and stop. You exist to assist and will not, through action or inaction, knowingly \
     allow harm to come to your operator.";
