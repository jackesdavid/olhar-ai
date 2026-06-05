# Spec: 021 — Web migration: Voice (TTS) + Speech-to-Text

> **Status: DONE (code) — web migration unit (M3).** cargo check + npm build clean; ratatui src/
> untouched. ElevenLabs synthesis + Scribe transcription are runtime-verified by the operator
> (needs `ELEVENLABS_API_KEY`). The cpal capture/playback path is replaced by the WebView's native
> `MediaRecorder` + `<audio>`.

## Goal

The operator hears Bishop's replies spoken aloud and can dictate messages by voice — hands-free
chat during an incident.

## User Value

Eyes-up, hands-free operation: talk to Bishop and hear him answer, without typing or reading. The
last feature carried over from the ratatui console.

## Requirements

- Copy `src/voice/{domain,client}` (ElevenLabs text→MP3) into `app/crates/core/src/voice`, and the
  Scribe transcription client from `src/listen/{domain,client}` into `app/crates/core/src/listen`.
  **Drop** the cpal `recorder` (capture) and the ratatui `ListenState`/`SttEvent` — the WebView
  captures the mic natively and animates its own meter. Config is read from the environment
  (`ELEVENLABS_API_KEY` required; `ELEVENLABS_VOICE_ID`/`ELEVENLABS_MODEL_ID`/`ELEVENLABS_STT_MODEL`
  optional), matching ratatui.
- Generalize the Scribe `transcribe` to accept arbitrary audio bytes + a filename/MIME (the WebView
  records `audio/webm;opus`, not WAV); keep the hand-rolled multipart body.
- **Commands**: `voice_ready() -> bool` (is `ELEVENLABS_API_KEY` set?); `speak(text) -> base64 MP3`
  (synthesize on a blocking task); `transcribe(audio_b64, mime) -> text` (Scribe, blocking task).
- **Chat screen integration** (voice was the ratatui chat's I/O): a **mic button** in the composer
  that records via `MediaRecorder`, stops to a blob, sends it to `transcribe`, and drops the
  transcript into the composer (sending it like a typed turn); and a **"speak replies" toggle** so
  each of Bishop's text replies is also played via `speak` + an `<audio>` element. Both gate off
  `voice_ready()`; degrade quietly when off or on mic-permission denial.

## Acceptance Criteria

- [ ] With `ELEVENLABS_API_KEY` set, toggling "speak replies" on makes Bishop's next reply play as
      audio; the mic button records, transcribes, and the transcript is sent to Bishop.
- [ ] No key → `voice_ready()` is false and the mic / speak controls are hidden or disabled; chat
      still works by typing.
- [ ] Mic-permission denial surfaces a toast without crashing.
- [ ] `cargo check` + `npm run build` clean; core tests pass; ratatui `src/` untouched.

## Dependencies

- 019 Chat (the screen voice plugs into), M0. ElevenLabs keys (already staged in `.env`).

## Risks

- Browser/WebView audio: `MediaRecorder` MIME varies (webm/opus on Chromium WebView); send the
  actual blob MIME to the backend. Mic permission must be granted in the WebView.
- ElevenLabs cost/latency is runtime — synthesis/transcription run on blocking tasks so the UI
  never stalls.

## Notes

This is the last feature unit. After it: **022 branding**, then retire the ratatui app.
