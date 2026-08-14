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

## Phase 1 — `ensure` mode on Control Tuya Device

A `mode` config on the existing node: `set` (today's behaviour, unchanged) or `ensure`.

In `ensure`, the node reads the point first and writes **only if it differs**, then reports:

- `ok` (Bool, existing)
- `changed` (Bool, new) — false means it was already in the wanted state

That is exactly "open or close based on the real state" for a **setpoint** like `door_control_1`,
and it makes the node idempotent: running the graph twice does not pulse anything twice.

## Phase 2 — pulse points need a state point

`ensure` cannot work on `gate1` alone: writing `true` is a momentary pulse, and reading it back
tells you nothing about the gate. The state lives in `gate1_fca` / `gate1_fcf`.

So `ensure` on a pulse point takes an optional **state code** and a **desired state**:

> *pulse `gate1` unless `gate1_fca` is already true*

Config: `state_code` (a read-only point from the same device) + `desired` (the value it should end
at). When `state_code` is unset, `ensure` falls back to reading the written point itself, which is
correct for a normal switch.

## Phase 3 — confirm it actually moved

The difference between "sent" and "done". A new **`Wait For State`** node:

- inputs: exec, plus config `tuya_device`, `tuya_code`, `value`, `timeout_secs`, `poll_secs`
- exec outputs: `reached` / `timeout`
- data output: the final observed value

A gate takes 10–20 seconds to travel. Polling `gate1_fca` until it is true — or giving up and
taking the `timeout` arm — is what lets the next node say "the gate is open" truthfully, and what
lets a workflow raise an alert when a gate is blocked.

`Wait` + a back-edge can already express polling, but not the timeout arm, and not without the
author hand-building a counter. This is a node because the pattern is universal and the failure
mode (a loop with no exit) is one people ship by accident.

**Engine note:** the run must not block on a physical device any more than it blocks on a person.
The suspend/resume machinery already exists twice — `DelayTrigger`'s trailing edge and Device
Gate's sweep (`docs/ref/workflows.md`). This is the third user, and it should reuse that shape
rather than invent a fourth.

## Phase 4 — the one-button graph

With the above, the whole journey is three nodes:

```
When Event(widget run) → Device Gate("Open the gate?") → Control(ensure, gate1, state gate1_fca)
                                                       → Wait For State(gate1_fca = true, 25s)
                                                          reached → Notify("Gate is open")
                                                          timeout → Notify("Gate did not move")
```

Ship a **template** for it, so the next person does not rebuild it from parts.

## Open decisions

1. **Is `Wait For State` Tuya-specific or generic?** Proposed: **Tuya-specific first**, because it
   needs to poll a device the engine has no generic notion of. A generic "wait until this Bool is
   true" would need to re-run its producer, which means re-entering an arbitrary subgraph — a much
   larger change to the scheduler. Revisit once a second provider exists.
2. **Should `ensure` be a mode, or a separate node?** Proposed: a mode. The device/point selectors
   and the typed value widget are the entire node; duplicating them to change one behaviour is how
   two nodes drift.
3. **What does `ensure` do when the state point disagrees with the setpoint** (a gate stopped
   half-way — neither `fca` nor `fcf` true)? Proposed: treat "not in the desired state" as "act",
   and let Phase 3 catch the case where it still does not arrive.
4. **Poll interval floor.** Every poll is a cloud round-trip (~300–800 ms, measured). Proposed
   floor 2 s, default 3 s — and this is the measured argument for local control (#133), where a
   poll is a LAN round-trip.

## Out of scope

- Local control. That is #133, and it changes the transport, not this logic.
- Any change to Device Gate. It already does its job.
- Multi-device orchestration ("close every gate").
