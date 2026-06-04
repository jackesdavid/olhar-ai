# Architecture Principles

These are reverse-engineered from the existing Bishop codebase. New specs must honor
them — the current state of the project is the reference standard.

## Core Principles

- **Vertical slices, one folder per feature.** Each feature lives in its own module
  (`scan/`, `chat/`, `map/`, `alerts/`, `notification/`, `voice/`, `listen/`, `stream/`,
  `config/`, `device/`, …).
- **`domain` / `handler` / `view` split inside each slice.** `domain.rs` holds models and
  state, `handler.rs` holds logic and key routing, `view.rs` holds rendering. Optional
  `client.rs` / `actions.rs` / `adapter.rs` isolate external IO or shared operations.
- **Never block the UI thread.** Every external call (Claude, ElevenLabs, Telegram, ONVIF,
  ffmpeg, mic) runs on a background thread and reports back over an `mpsc` channel.
- **Shared mutable state is `Arc<Mutex<T>>`**, used sparingly (device store, alert store,
  notification targets) and only where two threads genuinely share it.
- Prefer readability over cleverness; keep the theme/aesthetic intact.

## Delivery Philosophy

Prefer:
- small specs (one screen / one feature folder)
- iterative vertical slices that don't destabilize existing screens
- flat JSON persistence with explicit `load` / `save` functions
- channel-based concurrency over locks

Avoid:
- giant upfront architecture
- a database or async runtime (the app is sync + threads by design)
- speculative abstractions and premature optimization
- breaking `--tty` / `--no-default-features` builds (features must stay optional)

## AI-Native Principles

AI performs significantly better when:
- context is structured
- terminology is consistent (see `domain_glossary.md`)
- specs are focused and testable
- workflows are incremental

The `ai/` structure exists to improve AI consistency and collaboration. The application
code under `src/` is the source of truth — when a spec and the code disagree, the code
wins until a spec deliberately changes it.
