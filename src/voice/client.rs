//! The ElevenLabs HTTP client. Blocking on purpose — callers run it on a
//! detached background thread so it never touches the render loop.
use std::io::Read;
use std::time::Duration;

use super::domain::{VoiceConfig, SIMILARITY_BOOST, STABILITY};

/// Synthesize `text` into MP3 bytes, or `None` on any failure (missing key,
/// network error, non-200) so the caller degrades to silence.
pub fn synthesize(text: &str) -> Option<Vec<u8>> {
    let cfg = VoiceConfig::from_env()?;

    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}",
        cfg.voice_id
    );
    let body = format!(
        "{{\"text\":\"{}\",\"model_id\":\"{}\",\"voice_settings\":{{\"stability\":{},\"similarity_boost\":{}}}}}",
        json_escape(text),
        json_escape(&cfg.model_id),
        STABILITY,
        SIMILARITY_BOOST,
    );

    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
        .post(&url)
        .set("xi-api-key", &cfg.api_key)
        .set("Content-Type", "application/json")
        .set("Accept", "audio/mpeg")
        .send_string(&body)
        .ok()?;

    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Minimal JSON string escaping — enough for the short, controlled phrases we
/// send.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
