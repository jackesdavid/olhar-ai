# Architecture Notes — Boot Sequence

## Components

- `src/boot/domain.rs` — `BootSequence` state (line offset, current index, completed flag),
  hardcoded boot text.
- `src/boot/handler.rs` — line-by-line scrolling, audio beep timing, skip-on-keypress.
- `src/boot/view.rs` — ASCII-art rendering.
- `src/audio/` — beep / boot-loop playback (optional `audio` feature).
- `src/app/handler.rs` + `src/app/domain.rs` — `Screen::Boot` routing, transition out.

## APIs

None external. Consumes the `Sfx` trait from `src/audio/mod.rs`.

## Data Flow

`main` → app starts on `Screen::Boot` → `on_tick` advances the scroll and triggers the beep
via the audio backend → on completion (or any key) the screen transitions to splash/menu.

## Important Decisions

- Boot copy is hardcoded over configurable, because the aesthetic is fixed product identity.
- Audio is optional (feature-gated) so the boot still runs in silent/tty builds.

## Risks

- Beep/scroll timing is best-effort, not frame-synced — acceptable for a cosmetic sequence.
