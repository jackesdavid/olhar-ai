# Skill: Orchestrate

## Purpose

Execute the role-based delivery loop on a single spec, from drafted spec to reviewed implementation, with explicit validation gates and send-back rules.

This is the **playbook** invoked by the `/sdd-orchestrate` slash command and by anyone in the Orchestrator role (see `ai/orchestration/orchestrator.md`).

---

## Inputs

Before starting, confirm:
1. A spec exists at `ai/specs/<spec-id>/spec.md` with at least: Goal, Acceptance Criteria, Risks.
2. `ai/STATE.md` is current (or initialize it).
3. Mode is set: **HITL** (default) or **autonomous**. See `ai/orchestration/hitl_policy.md`.

If the spec doesn't exist yet, do not orchestrate — invoke `write_spec_socratic.md` or `write_spec.md` first.

---

## The loop

```
        [Analyst*]
            ↓
          [PM] ──► gate ──► [Architect] ──► gate ──► [Developer] ──► gate ──► [Tester] ──► gate ──► [Reviewer] ──► DONE
            ▲                  ▲                       ▲                       │                      │
            └──────────────────┴───────────────────────┴───────── send-back ◄──┴──── send-back ◄──────┘

* Analyst only runs if spec was vague/missing.
```

Each `gate` is a call to `validate_handoff.md`. Each `send-back` routes to the **shallowest** upstream role that can fix the issue (see decision tree in `validate_handoff.md`).

---

## Phase-by-phase procedure

For each phase below, the orchestrator:

1. **Loads** only the role file for the current phase (token economy — don't load all roles).
2. **Invokes** the role with the current spec and relevant context (`STATE.md`, prior handoff contract).
3. **Receives** the role's output.
4. **Validates** the output via `validate_handoff.md`.
5. **In HITL mode:** pauses for human approval at each gate.
6. **In autonomous mode:** logs the decision to `STATE.md` and proceeds, unless a send-back is required.
7. **Updates** `STATE.md` and advances to the next phase (or routes the send-back).

### Phase 1 — PM

- Role file: `ai/roles/pm.md`
- Goal: confirm/refine acceptance criteria, surface scope ambiguity, prioritize.
- Output: updated `spec.md` (or note "spec already PM-ready").
- Gate check: spec passes the PM gate in `validate_handoff.md`.

### Phase 2 — Architect

- Role file: `ai/roles/architect.md`
- Goal: produce `architecture.md` in the spec folder. Identify boundaries, risks, key tradeoffs.
- Output: `ai/specs/<spec-id>/architecture.md`.
- Gate check: architecture passes the Architect gate.

### Phase 3 — Developer

- Role file: `ai/roles/developer.md`
- Goal: implement against the spec + architecture. Update `tasks.md` as work progresses.
- Output: code + tests in `src/` (or wherever the host project lives) + updated `tasks.md`.
- **Recommendation:** for non-trivial implementations, delegate to a sub-agent so the main orchestration session stays lean.
- Gate check: implementation passes the Developer gate.

### Phase 4 — Tester

- Role file: `ai/roles/tester.md`
- Goal: verify every acceptance criterion is met. Run/write tests as needed.
- Output: test results + notes.
- Gate check: all acceptance criteria pass. If not — **send-back** per the decision tree.

### Phase 5 — Reviewer

- Role file: `ai/roles/reviewer.md`
- Goal: maintainability, spec completion, architectural consistency.
- Output: review notes; either DONE or a send-back.
- Gate check: reviewer signs off.

---

## Send-back logic (summary)

The full decision tree lives in `validate_handoff.md`. Quick reference:

| Failure | Route to |
|---|---|
| Spec ambiguous / missing AC | PM (or Analyst if foundational) |
| Architectural boundary violated | Architect |
| Implementation ≠ spec | Developer |
| Tests fail — implementation issue | Developer |
| Tests fail — design issue | Architect |
| Maintainability concerns | Developer |
| Requirement turns out wrong | PM → Analyst |

**Hard rule:** maximum 2 auto-iterations on the same gate. The third forces HITL even in autonomous mode.

---

## Updating `STATE.md`

After every phase transition, append one decision line and update the header fields. Keep `STATE.md` under ~50 lines — older decisions belong in git history or in `ai/specs/<spec-id>/notes.md`.

---

## DONE

When the Reviewer signs off:

1. Update `STATE.md`: `current_phase: done`.
2. Move the spec folder marker (or update its header) to indicate completion.
3. In HITL mode, ask the human: *"Spec complete. Move to next spec, or pause here?"*
4. In autonomous mode, look for the next spec in `ai/specs/` ordered by id; if none, stop.

---

## Token-economy reminders

- Load only the active role file for the current phase.
- Prefer sub-agents for execute/test phases.
- Keep `STATE.md` lean (rolling log, not full history).
- Don't re-load `ai/context/*` every phase — read it once at orchestration start and reference it.
- When in doubt, smaller context wins.
