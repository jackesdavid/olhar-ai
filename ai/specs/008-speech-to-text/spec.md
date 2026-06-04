# Spec: Speech-to-Text (Microphone → Scribe)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/listen/`.
> Documents existing behavior; no code change implied.

## Goal

The operator dictates a chat message by voice, transcribed via ElevenLabs Scribe.

---

## User Value

Hands-free input during an incident, so the operator can talk to Bishop without typing.

---

## Requirements

- A key (F9) starts microphone capture via cpal.
- Show a live amplitude meter while recording.
- Send captured audio to the ElevenLabs Scribe API on a background thread.
- Place the transcript into the chat input; auto-send if `stt_autosend` is enabled.
- Degrade gracefully when no mic / no API key / audio not compiled.

---

## Acceptance Criteria

- [x] Pressing the listen key starts recording and shows a moving level meter.
- [x] Stopping/finishing produces a transcript placed in the chat input.
- [x] With `stt_autosend` enabled, the transcript is sent automatically.
- [x] Transcription failures surface without crashing the console.
- [x] No microphone or missing key falls back gracefully.

---

## Dependencies

- 005-bishop-chat (transcript target)
- 012-config (`stt_autosend`)

---

## Risks

- Mic availability and permissions are platform-dependent.
- Scribe latency/quality is external.

---

## Notes

Default model `scribe_v1` (`ELEVENLABS_STT_MODEL`). `SttEvent` = Transcript | Failed.
Out of scope: streaming/partial transcripts.
