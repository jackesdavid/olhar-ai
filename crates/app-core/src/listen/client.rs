//! The ElevenLabs speech-to-text (Scribe) HTTP client. Blocking on purpose — the
//! caller runs it on a blocking task. ureq has no multipart helper, so we
//! assemble the `multipart/form-data` body by hand around a fixed boundary.
//!
//! Ported from the ratatui `src/listen/client.rs`; generalized to take the audio
//! bytes + a filename/MIME, since the WebView records `audio/webm;opus` rather
//! than WAV.
use std::io::Read;
use std::time::Duration;

use super::domain::SttConfig;

const ENDPOINT: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const BOUNDARY: &str = "----bishopFormBoundary7MA4YWxkTrZu0gW";

/// Transcribe an audio byte buffer to text, or `Err(reason)` on any failure
/// (missing key, network error, non-200, empty result). `filename`/`mime`
/// describe the audio container (e.g. `speech.webm` / `audio/webm`).
pub fn transcribe(audio: &[u8], filename: &str, mime: &str) -> Result<String, String> {
    let cfg = SttConfig::from_env().ok_or("no ELEVENLABS_API_KEY set")?;
    if audio.is_empty() {
        return Err("no audio captured".into());
    }

    let body = multipart_body(&cfg.model_id, audio, filename, mime);
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");

    let resp = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
        .post(ENDPOINT)
        .set("xi-api-key", &cfg.api_key)
        .set("Content-Type", &content_type)
        .set("Accept", "application/json")
        .send_bytes(&body)
        .map_err(|e| format!("request failed: {e}"))?;

    let mut text = String::new();
    resp.into_reader()
        .read_to_string(&mut text)
        .map_err(|e| format!("read failed: {e}"))?;

    // Pull the "text" field out of the JSON response.
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("bad response: {e}"))?;
    let transcript = value
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if transcript.is_empty() {
        return Err("nothing recognized".into());
    }
    Ok(transcript)
}

/// Build a `multipart/form-data` body with the `model_id` text field and the
/// `file` audio part.
fn multipart_body(model_id: &str, audio: &[u8], filename: &str, mime: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(audio.len() + 256);
    let mut push = |s: &str| body.extend_from_slice(s.as_bytes());

    push(&format!("--{BOUNDARY}\r\n"));
    push("Content-Disposition: form-data; name=\"model_id\"\r\n\r\n");
    push(model_id);
    push("\r\n");

    push(&format!("--{BOUNDARY}\r\n"));
    push(&format!(
        "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n"
    ));
    push(&format!("Content-Type: {mime}\r\n\r\n"));
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_has_both_parts_and_closing_boundary() {
        let body = multipart_body("scribe_v1", &[1, 2, 3, 4], "speech.webm", "audio/webm");
        let head = String::from_utf8_lossy(&body);
        assert!(head.contains("name=\"model_id\""));
        assert!(head.contains("scribe_v1"));
        assert!(head.contains("filename=\"speech.webm\""));
        assert!(head.contains("Content-Type: audio/webm"));
        assert!(head.trim_end().ends_with(&format!("--{BOUNDARY}--")));
    }
}
