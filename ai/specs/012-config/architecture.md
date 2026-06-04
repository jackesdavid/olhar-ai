# Architecture Notes — Ship Configurations

## Components

- `src/config/domain.rs` — `Settings` struct, `load`/`save` (`config.json`).
- `src/config/handler.rs` — cursor navigation, field editing.
- `src/config/view.rs` — settings list rendering.
- Live targets: `src/audio/levels.rs` (`AudioLevels` mixer), `src/ui/video.rs` /
  `src/crt/` (CRT post-processor), and stream/chat reading their respective fields.

## APIs

None external.

## Data Flow

Operator edits a field → `handler` updates the in-memory `Settings` and writes `config.json`
→ shared atomics in `AudioLevels` (gains, `voice_enabled`) and the CRT post-processor flags
update so playback and rendering reflect changes on the next frame. Stream/chat read
`transport` / `typing_speed` / `stt_autosend` when they act.

## Important Decisions

- Settings push into shared atomics so changes apply live without restart.
- Flat `config.json` mirrors the rest of the app's local-first persistence.

## Risks

- Cross-thread application of settings must use atomics / the shared mixer, not direct mutation.
