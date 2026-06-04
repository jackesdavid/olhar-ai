# Spec: Ship Configurations (Settings)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/config/`.
> Documents existing behavior; no code change implied.

## Goal

The operator tunes audio, typing, input, and CRT settings from a Configurations screen, with
changes applied live and persisted.

---

## User Value

One place to make the console comfortable for the environment (volume, pacing, transport),
so the operator isn't fighting defaults.

---

## Requirements

- Edit audio mixer gains: master, voice, ui, ambient (0.0–1.0).
- Toggle voice synthesis on/off.
- Set typing speed (chars/tick) and STT auto-send.
- Choose RTSP transport (TCP / UDP / Auto).
- Toggle CRT sweep and set its intensity.
- Apply changes live (audio mixer + GPU post-processor) and persist to `config.json`.

---

## Acceptance Criteria

- [x] Changing a volume updates playback immediately and persists.
- [x] Toggling voice on/off takes effect on the next reply.
- [x] Typing speed and STT auto-send changes apply to chat behavior.
- [x] RTSP transport selection is used by new streams.
- [x] CRT sweep toggle/intensity update the window live (when `crt` is active) and persist.

---

## Dependencies

none

---

## Risks

- Live-applying settings to the audio mixer and GPU must be thread-safe.

---

## Notes

`Settings { master, voice, ui, ambient, voice_enabled, typing_speed, stt_autosend, transport,
crt_sweep, crt_sweep_intensity }` → `config.json`. Settings here drive specs 004/005/007/008.
