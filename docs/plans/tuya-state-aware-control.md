# Open or close, depending on what the gate is actually doing

**Board card:** #184 State-aware device control — act on the real state, confirm it changed
**Goal:** One button that opens a closed gate and closes an open one, and a workflow that knows
which happened — instead of firing a command and hoping.

## What already works, and what it costs

Proven end to end on 2026-08-14: iPhone widget → Device Gate (Face ID) → `Control Tuya Device`
→ the gate opens. Everything in this plan builds on that; none of it is a rewrite.

The pieces for state-aware control already exist too. `Read Tuya Status` exposes `on` (Bool), `If`
branches on it, `Wait` plus an exec back-edge gives a polling loop. So today you can build:

```
Read Tuya Status(gate1_fca) → (on) → If → true:  Control(door_control_1 = close)
                                        → false: Control(door_control_1 = open)
```

Five nodes for one button, rebuilt by hand for every device, and it still only proves a command was
*sent*. Whether the gate moved is a different question, and the one that matters when the next node
is "tell the resident it is open".

## The device is the argument

The gate module publishes, via the thing model:

| code | dp | access | what it is |
|---|---|---|---|
| `gate1` | 101 | rw | **pulse** — writing `true` runs the motor; there is no "set to closed" |
| `door_control_1` | 6 | rw | **setpoint** — enum `open` / `close` |
| `gate1_fca` | 105 | **ro** | limit switch: fully open |
| `gate1_fcf` | 106 | **ro** | limit switch: fully closed |
| `gate1_status` | 110 | **ro** | numeric state |

Two different control shapes in one device, and the ground truth is a *third* set of points. That
is the whole problem in miniature: the thing you write is not the thing you read.

## Phase 0 — a correctness bug to fix first

`device_functions` now builds the catalog from every thing-model property **including the
read-only ones**, because I dropped `accessMode` when mapping them. So `Control Tuya Device`
currently offers `gate1_fca` in its dropdown, and writing it will fail at run time with a Tuya
error nobody can act on.

- Keep `accessMode` (`rw` / `ro`) per function in `functions_json`.
- `Control Tuya Device`'s `tuya_code` dropdown lists only writable points; `Read Tuya Status` lists
  everything. Both read the same catalog — only the filter differs
  (`sentinel-app/src/routes/workflows/[[id]]/+page.svelte`, the `tuya_code` selector).

This is small and independent. It should ship before anything below, because every phase here makes
the dropdowns matter more.

## The shape, decided 2026-08-14: separate nodes, generic wait

No `ensure` mode, no Tuya-specific waiting. Every step is its own node, so the author composes
them — which also means the pieces work for the next provider, and for conditions that have
nothing to do with a device.

The insight that makes this cheap: a wait needs to **re-read**, and re-reading is what an exec
back-edge already does. So the wait node does not poll anything itself. It has a **`retry` arm**
that the author wires back to whatever produces the condition:

```
Control(gate1) →exec  Read Tuya Status(gate1_fca) →exec  Wait Until(cond, 25s)
                            ▲                                   │ retry
                            └───────────────────────────────────┘
                                                        reached → Notify("Gate is open")
                                                        timeout → Notify("Gate did not move")
```

The Read node re-runs because the loop re-enters it (`memoize=false`, as loop bodies already
require). Nothing in the scheduler changes. `Wait Until` only sleeps, counts, and picks an arm.

## Phase 1 — the `Wait Until` node

- **Inputs:** exec, `condition` (Bool).
- **Config:** `timeout_secs` (default 30), `poll_secs` (default 3, floor 2).
- **Exec outputs:** `reached` · `retry` · `timeout`. Three arms, and the split is load-bearing in
  the same way Device Gate's is: `timeout` means *nobody proved it happened*, which is not the
  same as proving it did not.
- **Data output:** `waited_secs` (Num), so a graph can report how long the gate took.

Deliberately generic: the condition is any Bool. A dictionary lookup, an HTTP result, a person
count, a limit switch. Nothing about it knows what Tuya is.

**How it counts.** State lives on `workflow_node_state` keyed by node + run, like `Cooldown`
already does (`engine.rs`) — first entry stamps a deadline, each re-entry compares against it.
Without that the node cannot tell its first visit from its fiftieth, and every loop would run
forever.

**On the run budget:** `WF_MAX_STEPS` (10 000) is the only bound actually enforced —
`WF_MAX_RUNTIME` survives in `docs/ref/workflows.md` and in one old plan note, but not in
`engine.rs`. At a 3 s poll a 30 s wait is ten steps, so the budget is not the constraint here.
**Fix the stale doc** as part of this card rather than leaving a limit documented that does not
exist.

## The order matters: READ BEFORE THE GATE

The first sketch put the Device Gate first and decided afterwards. That is backwards. If the phone
is going to ask a person to approve something, the question has to name what will happen — "Fechar
o portão?" is a different decision from "Abrir o portão?", and a prompt that says neither is asking
someone to authorise a surprise.

So the state is read FIRST, and the answer picks the message:

```
When Event → Read Tuya Status(Portão 1 Sensor FCA)
                  │ on
                  ▼
                 If ── true  → Text("Fechar o portão?") ─┐
                    └─ false → Text("Abrir o portão?")  ─┴→ Device Gate → Control
```

**This already works.** `message` is a wired, required `Text` input on Device Gate
(`DEVICE_GATE_IN`), not a config literal — so a `Text` or `Format` node drives it and the push, the
approval screen and the run log all carry the same sentence. Nothing to build.

What Phase 0 adds is the ability to *pick the right points* in those two nodes.

## Phase 2 — nothing else is a new node

With separate nodes, "act on the real state" is already expressible and needs no new kind:

| want | nodes |
|---|---|
| open only if closed | `Read Tuya Status(gate1_fcf)` → `If` → `Control(gate1)` |
| toggle | `Read` → `If` → two `Control`s |
| ask a person, naming the action | `Read` → `If` → `Text` → `Device Gate` |
| confirm it moved | `Control` → `Read` → `Wait Until` (loop) |
| give up loudly | `Wait Until` → `timeout` → `Notify` |

That is the flexibility being asked for: the graph says what it does, and any piece can be
swapped for a different device, a different condition, or a different reaction.

## Phase 3 — a template, not a node

Ship the one-button gate graph as a **global workflow template** (`seed_templates.rs`), so the
pattern is a starting point rather than folklore. Templates are already the mechanism for this
(see [[global-workflow-templates]]).

## Open decisions

1. **Does `Wait Until` sleep in-run, or suspend?** Proposed: **sleep in-run** for v1 — a gate is
   10–25 s and the existing `Wait` node already blocks that way. If a use case wants minutes, the
   suspend/resume machinery exists twice over (`DelayTrigger`'s trailing edge, Device Gate's
   sweep) and this becomes its third user. Revisit then, not now.
2. **Does `retry` need to exist, or should the node loop internally?** Proposed: **`retry` as an
   arm.** An internal loop would have to re-run the producer subgraph itself, which is the
   scheduler change this design exists to avoid — and an explicit back-edge is visible on the
   canvas, where a hidden loop is not.
3. **Poll floor.** 2 s. Every retry is a cloud round-trip (300–800 ms, measured), and this is the
   measured argument for local control (#133) rather than an asserted one.

## Out of scope

- Local control. That is #133, and it changes the transport, not this logic.
- Any change to Device Gate, or to Control Tuya Device beyond Phase 0.
- Multi-device orchestration ("close every gate").
