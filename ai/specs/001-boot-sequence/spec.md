# Spec: Boot Sequence

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/boot/`.
> Documents existing behavior; no code change implied.

## Goal

On launch, the operator sees a Weyland-Yutani "MU-TH-UR 6000" self-test boot animation
before reaching the interface.

---

## User Value

Establishes the product identity and signals the console is alive and initializing, so the
operator trusts the system is ready before interacting.

---

## Requirements

- Scroll self-test lines one at a time with a teletype/scroll feel.
- Play an audio beep/hum in time with the sequence (when audio is available).
- End on a "READY FOR INQUIRY" state and transition into the app.
- Any key skips the sequence.

---

## Acceptance Criteria

- [x] On startup the boot lines render and scroll until complete.
- [x] A ready tone/beep plays at the end when the `audio` feature is active.
- [x] Pressing any key skips directly to the next screen.
- [x] With `--no-default-features` (no audio) the sequence still completes silently.

---

## Dependencies

none

---

## Risks

- Audio timing drift between the visual scroll and the beep (low; cosmetic).

---

## Notes

Out of scope: configurable boot text, multiple boot themes. The boot copy is hardcoded.
