# Architecture Notes — Speech-to-Text

## Components

- `src/listen/domain.rs` — `ListenState` (active flag, amplitude atomic, recording handle),
  `SttConfig`, `SttEvent`.
- `src/listen/recorder.rs` — microphone capture via cpal, amplitude smoothing.
- `src/listen/client.rs` — HTTP POST to ElevenLabs Scribe.
- Consumers: `src/chat/` (transcript → input), `src/app/handler.rs` (F9 routing,
  `stt_tx`/`rx`).

## APIs

- ElevenLabs Scribe (speech-to-text) REST API.

## Data Flow

F9 → recorder thread captures PCM via cpal, updating a shared atomic amplitude for the meter →
on stop, audio is POSTed to Scribe on a background thread → `SttEvent::Transcript` posts over
`stt_tx` → placed in chat input (auto-sent if `stt_autosend`).

## Important Decisions

- cpal capture + atomic amplitude lets the UI animate the meter without locking.
- Transcription is one-shot (capture then send) rather than streaming — simpler.

## Risks

- Platform mic permissions/availability; external Scribe latency.
