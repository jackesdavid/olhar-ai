# Current Milestone

## Current Focus

**Adopting Spec-Driven Development on an already-working app.**

Bishop is a mature, working console (boot, scan/discovery, device detail, live stream,
chat + tools, voice, STT, map, alerts, notifications, config). The current code is the
reference standard and must not be changed by this adoption work.

This milestone has two tracks:
1. **Retrospective specs** — write `ai/specs/NNN-*` records for existing features so the
   repository is coherent and future specs have prior art to depend on. These describe
   what already shipped; they do not change code.
2. **First forward spec** — the next *new* feature, authored and orchestrated through the
   SDD workflow (`/sdd-spec` → `/sdd-orchestrate`).

---

## Active Specs

- _none active._ **013-workflows-automation — DONE (v1)** — first forward feature shipped
  through the full SDD loop. Phase-2 follow-ups (cron interval editor, branching/loops, run
  history, camera_motion, multi-instance payloads) live in its `tasks.md` Notes.

### Retrospective records (shipped — documentation only, not active work)

- 001-boot-sequence · 002-camera-discovery · 003-device-detail · 004-live-stream
- 005-bishop-chat · 006-agent-tools · 007-voice-tts · 008-speech-to-text
- 009-floorplan-map · 010-alerts · 011-notifications · 012-config

---

## Risks

- Retrospective specs could drift from the actual code; they must be reverse-engineered
  from `src/`, not invented.
- A forward feature could destabilize existing screens — every new feature should remain a
  self-contained vertical slice.
- New external IO must stay off the UI thread (channels), or the console will stutter.

---

## Open Questions

- Which existing features get retrospective specs, and in what order? (all ~12 vs. core few)
- What is the first forward feature to build through SDD?
- Mode for the first orchestration run: HITL, mixed, or autonomous?
