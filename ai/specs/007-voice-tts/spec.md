# Spec: Voice Synthesis (Text-to-Speech)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/voice/` and
> `src/audio/`. Documents existing behavior; no code change implied.

## Goal

Bishop speaks its replies aloud using ElevenLabs text-to-speech, synced with the on-screen
teletype reveal.

---

## User Value

Hands-free situational updates and a stronger synthetic-character presence, so the operator
can keep eyes on feeds while Bishop talks.

---

## Requirements

- POST reply text to the ElevenLabs TTS API on a background thread.
- Stream the returned audio to the rodio playback backend.
- Fire an on-ready callback when playback starts so the text reveal stays in sync.
- Respect the audio mixer (master/voice gains) and a `voice_enabled` toggle.
- Degrade to silent mode when `ELEVENLABS_API_KEY` is missing or audio is not compiled.

---

## Acceptance Criteria

- [x] When voice is enabled, Bishop's reply is spoken via ElevenLabs.
- [x] Playback begins roughly in sync with the teletype reveal.
- [x] Disabling voice (config) silences speech without affecting text.
- [x] Missing API key / `--no-default-features` falls back to silent, no crash.
- [x] Voice volume follows the master/voice mixer gains.

---

## Dependencies

- 005-bishop-chat (provides the text to speak)
- 012-config (voice toggle + gains)

---

## Risks

- TTS latency could desync from the reveal — mitigated by the on-ready callback.
- ElevenLabs availability/quota is external.

---

## Notes

Defaults: voice "Adam" (`pNInz6obpgDQGcFmaJgB`), `eleven_multilingual_v2`. Configurable via
`ELEVENLABS_VOICE_ID` / `ELEVENLABS_MODEL_ID`. Speech-to-text is spec 008.
