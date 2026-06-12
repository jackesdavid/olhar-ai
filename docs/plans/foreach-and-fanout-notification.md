# ForEach + Fan-out Notification — Plan (Phase 5)

**Parent plan:** [[school-student-frequency]] (Phase 5)
**Use case:** [[school-frequence]]
**Goal:** notify every guardian of a recognised person by **composing generic, reusable nodes** —
no school-specific node. This is the dedicated decomposition of the heaviest phase; the engine
iteration design is the open question to settle before building.

## Target graph (all generic nodes)

```
CameraSnapshot → RecognizePerson → If (ok)
                                     └→ GetPersonParentRecipients → ForEach ──(item: Recipient)──┐
                                                                       │                          ▼
                                                                       │                 Format({name},{time})
                                                                       │                          ▼
                                                                       │                   NotifyRecipient   (exists)
                                                                       └→ (done) → …continue
```

## Work items (decomposed, buildable slices)

### A. `List` port type (collections flow on edges)
- `domain::PortType::List(Box<PortType>)` — a typed collection (here `List(Recipient)`). Update
  `add_edge` type-check (a `List(T)` source only fits a `List(T)` target; `ForEach` accepts `List(T)`
  and emits `T`), serde, and the editor port-colour map + catalog (`node_catalog`).
- `node::PortValue::List(Vec<PortValue>)` + an `as_list` accessor; literal handling (no literal for
  lists in v1 — they're produced by nodes).
- **Smallest first**: land the type plumbing with a trivial producer/consumer before the loop.

### B. `GetPersonParentRecipients` node (generic)
- In: a person `id` (Text) — or accept `person_id` from `RecognizePerson`. Out: `recipients`
  (`List(Recipient)`) + `count` (Num).
- Executor: reuse `persons::guardian_recipients(id)` (ancestors with `receive_notifications` + a
  recipient). DB-backed via `ToolContext` (people are in the in-memory store).

### C. `ForEach` / `Loop` node + **engine iteration** ⚠️ the hard part
- Node shape: in `list` (`List(T)`); exec outs **`body`** (fired once per element) and **`done`**
  (after the last); data out **`item`** (`T`, the current element).
- **Engine change** — today `run_dag_with` is a single pass over a topo order, each node running once,
  with exec-activation via `live_out`. Iteration needs the **body subgraph re-run per item** with
  `item` rebound each time. Decide the approach (see below) before coding.

### D. Wire the body
- Reuse existing `If`, `Format`, and **`NotifyRecipient`** (already sends to a `Recipient`). The loop
  `body` drives `Format → NotifyRecipient`; `item` feeds the recipient input.

### E. Tests
- `List` edge type-checks (accept `List(T)→List(T)`, reject `List(T)→T` and mismatches).
- `GetPersonParentRecipients` returns the right ordered recipients (hierarchy + permission).
- `ForEach` runs the body exactly N times with each `item`, then fires `done` once; empty list →
  only `done`; respects the iteration cap.

## Open design question — how the engine runs a loop body (decide first)

The current engine is acyclic/single-pass. Candidate approaches:
1. **Subtree re-execution.** When `ForEach` fires, the engine identifies the exec-descendants reachable
   from `body` (up to a join/`done`) and runs that sub-DAG once per item, rebinding `item`. Cleanest
   mental model; needs the engine to scope a "body region" and re-run it.
2. **Bounded unrolling.** Expand the body inline N times at run start. Simple, but N must be known and
   it bloats the run; awkward for data-driven counts.
3. **Iterator + queue.** Model the run as a work-queue instead of one topo pass, so a node can enqueue
   its body region per item. Most general, biggest rewrite of `engine.rs`.

Leaning **#1**. Pick after a closer read of `workflows/engine.rs` (the `live_out` exec-activation +
input resolution). Guardrails regardless: a hard **iteration cap** (e.g. 1000) and **no nested loops**
in v1 unless #1 falls out for free.

## Risks / notes

- Re-entrancy: a body that mutates shared state (alerts, notifications) runs N times — intended here
  (one notify per guardian), but document it.
- Keep `agent/tools.rs` in sync only if any node is exposed to the chat agent (likely not).
- All nodes stay generic; the school use case remains *only* a saved graph (Phase 7).
