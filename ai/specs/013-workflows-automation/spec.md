# Spec: Workflows — Graph-Based Automation

> **Status: DONE (v1) — shipped via SDD on 2026-06-04.** The first forward feature carried
> end-to-end through the SDD loop (spec → architect → developer → tester → done). Source:
> operator plan `great-lets-plan-and-crispy-goose.md`, reshaped in the Architect phase
> (typed ports, atomic nodes, engine-level triggers, named graphs). Implemented in
> `src/workflows/` (domain/node/engine/handler/view) + app wiring; 73 unit tests; Run-now
> path verified live (Test A) and link validation in-app (Test C). Phase-2 items and known
> v1 debt are tracked in `tasks.md`.

## Goal

The operator builds named automation graphs in which a trigger fires a chain of typed,
single-responsibility nodes that run once to completion.

---

## User Value

Turns Bishop's discrete capabilities (alert, notify, snapshot, describe) into hands-off
automations the operator wires once — so routine responses happen on their own instead of
being driven manually or by the AI every time.

---

## Requirements

- A WORKFLOWS screen holding **multiple named graphs** (create / rename / delete / select),
  each an independent automation persisted to `workflows.json`.
- Each graph has an **engine-level trigger** (`Manual` / `Cron{every_secs}` /
  `OnNewAlert{min_level}`), an `enabled` switch, and a per-graph **`ConcurrencyPolicy`**
  (`SkipIfRunning` / `QueueSingle{max_queued}` / `Concurrent{max_concurrent}`).
- A node **canvas editor** (mirroring the MAP editor) to add / move / configure / delete
  nodes and **link typed ports** (output → input) with type-checking.
- An **atomic node set** — source `get_last_alert`; actions `create_alert`, `deliver_alert`,
  `camera_snapshot`, `describe_image`, `notify_beacon`, `format`. Each does one thing;
  behavior is composed by linking. Actions are thin adapters over the **existing** agent tool
  registry — no new IO code.
- **Typed ports + edge mapping (isolation invariant):** a node receives only its declared
  input ports (a wired upstream output, or a config literal) and returns only its declared
  outputs; it never references another node. The **engine** owns all edges and resolves wiring.
- A **fire-and-forget engine**: when a graph's trigger fires, it runs the whole DAG **once**
  in topological order on a background thread (stop-on-error, cycle-guarded), honoring the
  graph's `ConcurrencyPolicy`.
- A manual **Run-now** on the selected graph.

---

## Acceptance Criteria

- [ ] Operator can create, rename, delete, and switch between named graphs; each graph's
      nodes/edges/trigger/concurrency/enabled round-trip through `workflows.json` across restart.
- [ ] Operator can add/move/delete nodes and link an output port to a **type-compatible** input
      port; linking a mismatched type or an already-fed input port is rejected.
- [ ] A graph with `Trigger=Cron{N}` and a `create_alert` node, once enabled, creates an alert
      every N seconds (visible on the ALERTS screen); Run-now fires it once on demand.
- [ ] A graph with `Trigger=OnNewAlert` and `get_last_alert → notify_beacon` sends a Telegram
      message built from the alert when a new alert appears (offline variant: `get_last_alert →
      create_alert` logs a follow-up alert).
- [ ] A node receives only its declared inputs and never references another node; a run executes
      each node at most once in topological order and stops on the first error.
- [ ] Each graph's `ConcurrencyPolicy` is honored (a fire while a run is in flight skips /
      queues ≤`max_queued` / runs concurrently ≤`max_concurrent`); node IO runs off the UI tick
      and PROCESSING lights during a run, in both TTY and CRT modes.

---

## Dependencies

- 006-agent-tools (the tool registry + `ToolContext` the actions adapt; `describe_image`
  adapts `src/agent/vision.rs`)
- 010-alerts (`AlertStore`, `actions::create`/`deliver`, the new-alert watcher source for
  `OnNewAlert` and `get_last_alert`)
- 011-notifications (`dispatch`, beacons targeted by `notify_beacon`)
- 009-floorplan-map (the canvas-editor pattern this mirrors)

---

## Risks

- **Action→trigger feedback loop:** an `OnNewAlert` graph containing a `create_alert` node
  re-fires across runs. Mitigation: the watcher **ignores `source == "workflow"`**;
  `create_alert` defaults `source = "workflow"`.
- **`get_last_alert` race:** it returns the most-recent alert, not provably the exact alert
  that fired the run. Acceptable v1; Phase 2 can pass the triggering id.
- **Typed-port editor complexity:** port selection + type-checking + graph switcher are the
  main cost. Mitigation: index-based selection (mirrors the MAP cursor), not hit-testing.
- **Blocking the 50 ms tick:** all node IO runs on the spawned run thread; `poll_workflows`
  only polls + drains channels; per-graph `ConcurrencyPolicy` + a small global cap bound load.

---

## Notes

**v1 is fire-and-forget, run-once, topological** — a fire runs the whole DAG once, then
discards run state. **No conditionals, loops, or branching.** Triggers are engine/graph
properties, **not nodes**. `Image` port values live only in-memory (never persisted). New
module `src/workflows/` mirrors `src/map/`; tests never write disk (`if cfg!(test) { return }`)
and an in-memory constructor is provided.

**Deferred to Phase 2:** long-running / streaming nodes; graphs that persist or stay live
across ticks; conditional branching / fan-out; loops; run history/log; `camera_motion`
trigger; capturing the exact triggering payload for queued/concurrent runs.
