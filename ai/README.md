# Empirical SDD+DDD — AI Engineering Scaffold

This directory is the heart of the starter. Copy it (plus `.claude/`) into any repository to turn that repo into a Spec-Driven + Document-Driven project.

---

## What's in here

```
ai/
├── STATE.md              ← single state file (the only always-loaded file at runtime)
├── context/              ← your project's context (fill once, ~15 min)
├── roles/                ← role definitions, one file per role
├── skills/               ← reusable playbooks (write_spec, orchestrate, …)
├── orchestration/        ← workflow, handoff rules, HITL policy, context policy
├── templates/            ← prompt templates for non–Claude Code environments
├── contracts/templates/  ← structured handoff / question / feedback contracts
└── specs/                ← one folder per spec; `_template/` is the starting point
```

---

## Where to start reading

If you have **5 minutes**: read `../README.md` and this file.
If you have **15 minutes**: add `orchestration/workflow.md` and `skills/orchestrate.md`.
If you have **30 minutes**: add `skills/write_spec_socratic.md`, `skills/validate_handoff.md`, and one or two role files.

You don't need to read everything to get started. The framework is designed to be picked up on demand.

---

## Core ideas

- **Specs are first-class.** Every meaningful unit of work has a folder under `specs/`.
- **Roles separate concerns.** Analyst frames, PM sharpens, Architect designs, Developer builds, Tester verifies, Reviewer signs off.
- **Gates catch problems early.** Every role-to-role handoff runs a small checklist (`validate_handoff.md`).
- **Send-backs are explicit.** When a gate fails, work routes to the shallowest role that can fix it.
- **Human-in-the-loop by default.** Autonomous mode is opt-in, and even then it stops after 2 failed gate iterations.
- **One state file.** `STATE.md` is the rolling truth — older history goes to git.
- **Token-cheap at runtime.** Load only the role file for the active phase; everything else is on demand.

---

## Two entry points

Pick the one that matches your situation:

- **Vague idea / stakeholder ask** → `/sdd-spec-socratic` (BA-style five-pass interview)
- **Clear ask** → `/sdd-spec` (direct authoring)

Then in both cases: `/sdd-orchestrate` to drive the spec through the role loop.

`/sdd-status` shows where you are. `/sdd-handoff` produces a structured handoff contract.

---

## Sizing

- 3–8 specs per milestone for small/medium projects.
- One spec ≈ one screen of `spec.md`.
- A spec moves through the loop in hours of human time, less if autonomous.

---

## Philosophy

Prefer:
- clarity, maintainability, simplicity
- small incremental delivery
- explicit handoffs and gates

Avoid:
- overengineering, speculative abstractions
- giant specs, multi-week umbrellas
- silent autonomous loops

---

## What this is not

- Not a framework (no install, no runtime dependency)
- Not BMAD (no 12+ personas, no Party Mode, no enterprise track)
- Not GSD (no 6 state files, no npm CLI)

It's a markdown scaffold. Copy it. Fill `context/`. Run a slash command. That's the whole thing.
