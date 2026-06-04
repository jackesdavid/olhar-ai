# Skill: Validate Handoff

## Purpose

Before any role hands work to the next, **validate** the artifact against a small, role-specific gate. If it fails, **send it back** to the shallowest role that can fix it. This is the core feedback loop of the framework.

Invoke this skill at every phase transition in `orchestrate.md`.

---

## How a gate works

A gate is a small checklist (3–6 items) that the artifact must pass. You answer each item yes/no. If any item is "no" → the artifact fails the gate.

Gates are intentionally cheap to run: ~30 seconds of reading + 5 yes/no decisions. Don't over-engineer them.

---

## Gate checklists

### PM gate (entry to Architect)

The spec is ready for architecture if:

- [ ] Goal is one sentence and names a user action
- [ ] Acceptance criteria are observable (testable) — no vague verbs
- [ ] In-scope and out-of-scope are explicit
- [ ] Dependencies on prior specs are listed (or "none")
- [ ] At least one risk is named (even if low)

### Architect gate (entry to Developer)

The architecture is ready for implementation if:

- [ ] Components / modules / files to touch are named
- [ ] Boundaries are explicit (what changes, what doesn't)
- [ ] Key tradeoff is documented (chose X over Y because Z)
- [ ] At least one risk has a mitigation note
- [ ] No speculative abstractions (YAGNI check)

### Developer gate (entry to Tester)

The implementation is ready for testing if:

- [ ] Every acceptance criterion has corresponding code
- [ ] Code follows existing project conventions (no surprise patterns)
- [ ] `tasks.md` checkboxes are honest (no false positives)
- [ ] Any deviation from `architecture.md` is documented in a handoff note

### Tester gate (entry to Reviewer)

Testing is complete if:

- [ ] Every acceptance criterion has a pass/fail verdict
- [ ] Failures are reproducible (steps recorded)
- [ ] Test scope matches the spec (not over-testing, not under-testing)

### Reviewer gate (DONE)

The spec is complete if:

- [ ] All acceptance criteria pass
- [ ] No unaddressed risks remain
- [ ] Code is maintainable (would a new dev understand it in 10 minutes?)
- [ ] No technical debt introduced beyond what's documented

---

## Send-back decision tree

When a gate fails, route to the shallowest role that can fix the issue. Don't kick everything back to PM by default — it wastes everyone's time.

```
Gate failed. Ask: WHO can fix this in the fewest changes?

├─ Missing/vague acceptance criteria? ────────► PM
│      └─ Foundational requirement unclear? ──► Analyst
├─ Architectural boundary issue? ─────────────► Architect
├─ Implementation doesn't match spec? ────────► Developer
├─ Tests reveal logic bug? ───────────────────► Developer
├─ Tests reveal design flaw (can't fix without redesign)? ──► Architect
├─ Maintainability / readability issue? ──────► Developer
└─ Requirement turned out to be wrong? ───────► PM (then maybe Analyst)
```

When sending back, **always** produce a handoff note (see `ai/contracts/templates/handoff_contract.md`) that:
- names the gate item that failed
- quotes the specific evidence
- proposes a fix direction (without prescribing the implementation)

---

## Loop limits

To prevent infinite ping-pong:

- **Max 2 auto-iterations** on the same gate in autonomous mode. The third forces HITL.
- If a send-back loops the same role twice without progress, escalate one level up (Dev → Architect → PM → Analyst → Human).

Log every send-back in `STATE.md` so the loop is visible.

---

## HITL vs autonomous

- **HITL:** the orchestrator presents the gate result to the human and asks "approve, or send back to X?"
- **Autonomous:** the orchestrator applies the decision tree itself and logs the decision. The human can still intervene at any time.

See `ai/orchestration/hitl_policy.md` for the full mode definition.

---

## Anti-patterns

- ❌ Treating gates as bureaucracy — they're for catching real problems, not for ceremony.
- ❌ Sending everything back to PM (lazy routing).
- ❌ Approving a gate with caveats ("good enough, but..."). If there's a but, the gate failed.
- ❌ Skipping the gate because "it obviously passes." Run it anyway — it's 30 seconds.
