# Human-in-the-Loop Policy

The framework supports two modes. **HITL is the default** — explicit human approval is required at every gate. `autonomous` is the opt-in escape hatch for trusted, well-scoped runs.

---

## How to set the mode

Mode is set in `ai/STATE.md` under `mode:`. It can be flipped at any time.

To start an autonomous run, the human says one of:
- *"orchestrate this autonomously"*
- *"run autonomous on spec NNN"*
- *"set mode autonomous"*

To return to HITL, say *"pause"* or *"HITL on"*.

(There is no CLI flag — this is a markdown convention, parsed by the orchestrator from the user's natural-language instruction.)

---

## HITL mode (default)

At every gate (`validate_handoff.md`), the orchestrator:

1. Runs the gate checklist.
2. Presents the result to the human (pass + summary, or fail + which item).
3. Asks: *"Approve and advance to <next role>? Or send back to <suggested role>?"*
4. Waits for human response. Does not advance silently.

Gates where HITL pauses:
- After spec is drafted (before PM)
- After PM refines spec (before Architect)
- After Architect produces architecture (before Developer)
- After Developer implements (before Tester)
- After Tester verifies (before Reviewer)
- After Reviewer signs off (before DONE / next spec)

---

## Autonomous mode

The orchestrator applies the gate + send-back decision tree itself, **and**:

1. **Logs every decision** to `STATE.md` (rolling 5-entry log).
2. **Stops on any send-back** the first two times it happens on the same gate — does not loop silently.
3. **Forces HITL** on the third iteration of the same gate (hard rule, regardless of mode).
4. **Pauses** if a role asks a question via `ai/contracts/templates/question_contract.md` — questions are always for humans.
5. **Reports** a one-line summary at the end of each phase so the human can scan progress.

Autonomous mode is appropriate when:
- The spec is small and well-scoped.
- The human is available to glance at progress occasionally.
- The risk of a wrong call at a gate is low.

It is **not** appropriate when:
- The spec is novel or touches sensitive areas.
- Acceptance criteria are still being negotiated.
- The team is new to the framework — run HITL until you trust it.

---

## Mixed mode (recommended for first runs)

A common pattern: start HITL through the Architect gate, then flip to autonomous for Developer + Tester, then return to HITL for Reviewer.

This concentrates human attention where decisions matter most (problem framing, design) and lets the AI move fast on mechanical phases.

---

## Anti-patterns

- ❌ Setting autonomous and walking away for hours — at minimum, glance at `STATE.md`.
- ❌ Approving HITL gates without reading — defeats the point of the gate.
- ❌ Flipping to autonomous mid-send-back to skip a fix.
