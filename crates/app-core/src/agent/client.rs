//! The Anthropic Messages API client with tool use. Blocking on purpose —
//! callers run it on a background task so it never touches the UI. Copied from
//! the ratatui `src/agent/client.rs`; the only change is that `ask` takes a
//! `voice` flag so the web chat uses the text-reply instruction instead of the
//! spoken-interface one.
use std::time::Duration;

use serde_json::{json, Value};

use super::domain::{
    text_of, AgentConfig, AgentOutcome, Message, CHAT_INSTRUCTION, TOOL_HINT, VOICE_INSTRUCTION,
};
use super::tools::{ToolContext, ToolRegistry};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024; // text replies can be a touch longer than spoken ones
const MAX_TOOL_ITERS: usize = 6; // cap the tool-use loop

/// Ask Claude for Bishop's reply, running any tools he calls. Returns `None` if
/// the API isn't configured or the request fails (caller uses the offline
/// persona). On success returns the final text plus every message produced
/// during the turn, to splice into the conversation history. `voice=false`
/// selects the text-console instruction (the web chat); `voice=true` keeps the
/// ratatui spoken-interface behaviour.
pub fn ask(
    history: &[Message],
    registry: &ToolRegistry,
    ctx: &ToolContext,
    voice: bool,
) -> Option<AgentOutcome> {
    let cfg = AgentConfig::from_env()?;

    // System prompt = dynamic personality + reply-style + tool etiquette.
    let style = if voice { VOICE_INSTRUCTION } else { CHAT_INSTRUCTION };
    let system = format!("{}{}{}", super::domain::load_personality(), style, TOOL_HINT);
    let tools_json = registry.specs_json();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();

    // Running message list for the request, and the new turns to return.
    let mut convo: Vec<Value> = history
        .iter()
        .map(|m| json!({ "role": m.role.as_str(), "content": m.content }))
        .collect();
    let mut appended: Vec<Message> = Vec::new();

    for _ in 0..MAX_TOOL_ITERS {
        let body = json!({
            "model": cfg.model,
            "max_tokens": MAX_TOKENS,
            "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
            "messages": convo,
            "tools": tools_json,
        });

        let resp = agent
            .post(ENDPOINT)
            .set("x-api-key", &cfg.api_key)
            .set("anthropic-version", ANTHROPIC_VERSION)
            .set("content-type", "application/json")
            .send_string(&body.to_string())
            .ok()?;
        let parsed: Value = serde_json::from_str(&resp.into_string().ok()?).ok()?;
        let content = parsed.get("content")?.clone();
        let stop = parsed.get("stop_reason").and_then(Value::as_str);

        // Record the assistant turn verbatim (preserves tool_use blocks).
        convo.push(json!({ "role": "assistant", "content": content }));
        appended.push(Message::assistant_raw(content.clone()));

        if stop != Some("tool_use") {
            return Some(AgentOutcome {
                reply_text: text_of(&content).trim().to_string(),
                appended,
            });
        }

        // Execute every tool_use block → tool_result blocks.
        let mut results = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let id = block.get("id").and_then(Value::as_str).unwrap_or("");
            let name = block.get("name").and_then(Value::as_str).unwrap_or("");
            let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
            let result = registry.run(name, &input, ctx);
            results.push(match result {
                Ok(s) => json!({ "type": "tool_result", "tool_use_id": id, "content": s }),
                Err(e) => {
                    json!({ "type": "tool_result", "tool_use_id": id, "content": e, "is_error": true })
                }
            });
        }
        convo.push(json!({ "role": "user", "content": results }));
        appended.push(Message::tool_results(results));
    }

    // Hit the iteration cap — return a terse final rather than spinning.
    Some(AgentOutcome {
        reply_text: "I wasn't able to finish that just now.".into(),
        appended,
    })
}
