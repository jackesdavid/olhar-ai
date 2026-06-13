//! Chat — Tauri adapter for the agent slice (Bishop): a single ongoing conversation
//! and the tool-use loop. The agent, tool registry, and client live in
//! `app_core::agent`; tool side-effects stream to the UI via `build_run_context`.
use std::sync::{Arc, Mutex};

use tauri::Manager;

/// The chat conversation history (with verbatim `tool_use`/`tool_result`
/// blocks), held in managed state for a single ongoing conversation.
pub type ChatHistory = Arc<Mutex<Vec<app_core::agent::Message>>>;

// ---- lifecycle (called by lib.rs setup) -------------------------------------

/// Manage an empty conversation history.
pub fn manage(app: &tauri::AppHandle) {
    let history: ChatHistory = Arc::new(Mutex::new(Vec::new()));
    app.manage(history);
}

// ---- commands ---------------------------------------------------------------

/// Whether the agent is configured (an `ANTHROPIC_API_KEY` is set). The chat
/// screen shows an offline notice when this is false.
#[tauri::command]
pub fn chat_ready() -> bool {
    app_core::agent::AgentConfig::from_env().is_some()
}

/// Clear the conversation history.
#[tauri::command]
pub fn reset_chat(history: tauri::State<'_, ChatHistory>) {
    history.lock().unwrap().clear();
}

/// Run one chat turn: append the operator's message, run Bishop's tool-use loop
/// on a blocking task, splice the produced turns into history, and return the
/// final reply. Tool side-effects (snapshots, store changes, open-live) stream
/// to the UI as Tauri events drained off the tool channel while the turn runs.
#[tauri::command]
pub async fn send_chat(
    app: tauri::AppHandle,
    history_state: tauri::State<'_, ChatHistory>,
    text: String,
) -> Result<String, String> {
    use app_core::agent::{client, Message, ToolRegistry};

    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Say something first.".into());
    }
    if app_core::agent::AgentConfig::from_env().is_none() {
        return Err("Bishop is offline — set ANTHROPIC_API_KEY to enable chat.".into());
    }

    // Optimistically append the user turn and snapshot the full history for the call.
    let snapshot = {
        let mut h = history_state.lock().unwrap();
        h.push(Message::user(text));
        h.clone()
    };

    // Shared tool context: side-effects stream to the UI while the turn runs.
    let ctx = crate::build_run_context(&app).ok_or("Sentinel's database isn't ready yet.")?;

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let registry = ToolRegistry::new();
        client::ask(&snapshot, &registry, &ctx, false)
    })
    .await
    .map_err(|e| format!("Chat task failed: {e}"))?;

    match outcome {
        Some(o) => {
            history_state.lock().unwrap().extend(o.appended);
            Ok(o.reply_text)
        }
        None => {
            // The request failed — roll back the optimistic user turn so history
            // stays valid (no two user turns in a row on the next request).
            history_state.lock().unwrap().pop();
            Err("Bishop didn't respond just now — check the API key and network, then retry.".into())
        }
    }
}
