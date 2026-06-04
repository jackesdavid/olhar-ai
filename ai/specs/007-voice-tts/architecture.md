# Architecture Notes — Voice Synthesis (TTS)

## Components

- `src/voice/domain.rs` — `VoiceConfig` (api_key, voice_id, model_id from env).
- `src/voice/client.rs` — HTTP POST to ElevenLabs TTS.
- `src/audio/adapter.rs` — rodio playback backend, speaking channels.
- `src/audio/levels.rs` — `AudioLevels` shared mixer (master/voice/ui/ambient + voice_enabled).
- `src/audio/mod.rs` — `Sfx` trait, `init()` builder, `Silent` fallback.

## APIs

- ElevenLabs Text-to-Speech REST API.

## Data Flow

Bishop reply text → `speak()` spawns a thread that POSTs to ElevenLabs → audio bytes stream
to the rodio backend → on-ready signal posts over the `display_tx` channel so the chat reveal
starts in sync. Playback frames read current gains from the shared `AudioLevels`.

## Important Decisions

- Background TTS + on-ready callback keeps audio and text in sync without blocking.
- A `Silent` implementation of `Sfx` keeps the app working with audio disabled.

## Risks

- External TTS latency/quota; mitigated by async playback and graceful silent fallback.
