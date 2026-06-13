//! Voice — Tauri adapter for the speech slice: text-to-speech (ElevenLabs) and
//! speech-to-text (Scribe). Stateless. The synthesis lives in `app_core::voice`;
//! transcription in `app_core::listen`.
use base64::{engine::general_purpose::STANDARD as B64, Engine};

/// Whether voice is configured (an `ELEVENLABS_API_KEY` is set). The chat screen
/// hides/disables the mic + speak controls when false.
#[tauri::command]
pub fn voice_ready() -> bool {
    app_core::voice::VoiceConfig::from_env().is_some()
}

/// Synthesize `text` to speech and return base64 MP3 for the WebView to play.
#[tauri::command]
pub async fn speak(text: String) -> Result<String, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Nothing to speak.".into());
    }
    let mp3 = tauri::async_runtime::spawn_blocking(move || app_core::voice::synthesize(&text))
        .await
        .map_err(|e| format!("Speak task failed: {e}"))?;
    match mp3 {
        Some(bytes) => Ok(B64.encode(&bytes)),
        None => Err("Voice synthesis failed — check ELEVENLABS_API_KEY and network.".into()),
    }
}

/// A filename for the Scribe multipart part, derived from the recorded MIME.
fn audio_filename_for(mime: &str) -> String {
    let ext = match mime.split(';').next().unwrap_or("").trim() {
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/aac" => "mp4",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => "webm",
    };
    format!("speech.{ext}")
}

/// Transcribe a base64-encoded audio clip (recorded in the WebView) via Scribe.
#[tauri::command]
pub async fn transcribe(audio_b64: String, mime: Option<String>) -> Result<String, String> {
    let bytes = B64.decode(audio_b64.as_bytes()).map_err(|e| format!("Bad audio data: {e}"))?;
    let mime = mime.unwrap_or_else(|| "audio/webm".into());
    let filename = audio_filename_for(&mime);
    let out = tauri::async_runtime::spawn_blocking(move || {
        app_core::listen::transcribe(&bytes, &filename, &mime)
    })
    .await
    .map_err(|e| format!("Transcribe task failed: {e}"))?;
    match &out {
        Ok(t) => log::info!("transcribe: {} chars", t.len()),
        Err(e) => log::warn!("transcribe failed: {e}"),
    }
    out
}
