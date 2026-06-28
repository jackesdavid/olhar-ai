# TimeGate Node — Implementation Plan

**Board card:** TimeGate workflow node — debounced motion wait
**Goal:** A workflow node that starts a countdown on first trigger and extends it on subsequent triggers using the formula `max(0, extend_secs - remaining)` — so re-triggers only extend the wait when less than `extend_secs` remains.

---

## The formula

```
on re-trigger:
  increase  = max(0, extend_secs - remaining_ms / 1000)
  new_remaining = remaining + increase  (= max(remaining, extend_secs))
```

**Example 1** — wait 10s, 5s elapsed (5s remaining), extend_secs = 10:
- increase = max(0, 10 - 5) = 5s
- new_remaining = 5 + 5 = 10s
- total from start = 5 + 10 = **15s**

**Example 2** — wait 20s, 5s elapsed (15s remaining), extend_secs = 10:
- increase = max(0, 10 - 15) = 0
- no extension → wait out the 15s

The guarantee: after any re-trigger, **at least `extend_secs` seconds remain** before the gate fires.

---

## Node shape

```
Category: Control
Label: Time Gate
Slug: time_gate

Input ports:
  arm    (Exec) — starts/extends the countdown
  reset  (Exec) — cancels the countdown (optional, nice-to-have v1)

Output ports:
  out    (Exec) — fires when countdown expires, no remaining extensions pending

Config:
  wait_secs:   u32  (initial countdown, default 10)
  extend_secs: u32  (minimum remaining on re-trigger, default = wait_secs)
```

`extend_secs` defaults to `wait_secs` so a single config value covers most use cases.
The `reset` port can be wired to a "manual override stop" path; if not wired, the node ignores it.

---

## State machine

Stored in `workflow_node_state` (key = `(graph_id, node_id, "time_gate")`), JSON value:

```json
{
  "state": "idle" | "armed",
  "expires_at": "2026-06-23T14:32:10Z"   // RFC3339, absent when idle
}
```

Transitions:
- `idle` + `arm` trigger → write `state=armed, expires_at=now+wait_secs`, start polling
- `armed` + `arm` trigger → recompute `expires_at = max(expires_at, now+extend_secs)`, update DB
- `armed` + timer expires → write `state=idle`, emit `out`
- `armed` + `reset` trigger → write `state=idle`, no output

---

## Cross-run operation (key design point)

Two workflow runs may touch the same TimeGate concurrently:
- **Run A** (the arming run): polls the node state in a tight loop (check every 250ms).
- **Run B** (a re-trigger run from a second motion event): hits the `arm` port, extends `expires_at`
  in the DB, then completes immediately (no blocking).

Run A's polling loop reads the new `expires_at` from DB on each iteration and continues sleeping.
When `now >= expires_at`, run A emits `out` and the downstream path (e.g. StopRecording) fires.

This means TimeGate's `arm` input has **two behaviors**:
1. If state = idle → start countdown (blocking run, polls until expiry, then emits `out`).
2. If state = armed → extend only (non-blocking, returns immediately — no `out` from this run).

The caller graph wires both `OnEvent("motion")` paths to `TimeGate.arm`:
```
OnEvent("motion") → StartRecording → TimeGate.arm
                                       ↓ (out, when countdown expires)
                                     StopRecording
```
Each new motion fires a new graph run. The first run becomes the "keeper" (polls to expiry);
subsequent runs extend and exit. This is the natural cross-graph event model already in use.

---

## Implementation — `crates/app-core/src/workflows/`

### `domain.rs`

```rust
TimeGate,
// in ALL:
NodeKind::TimeGate,
// ports + config:
"time_gate" => NodePorts {
    exec_in:  &["arm", "reset"],
    exec_out: &["out"],
    data_in:  &[],
    data_out: &[],
},
// default_config:
NodeKind::TimeGate => json!({ "wait_secs": 10, "extend_secs": 10 }),
// label/category:
NodeKind::TimeGate => ("Time Gate", "Control"),
```

### `node.rs`

```rust
NodeKind::TimeGate => run_time_gate(ctx, node, inputs).await,
```

`run_time_gate(ctx, node, inputs)`:
1. Read `triggered_port` from `inputs.exec_port` (default "arm").
2. If triggered_port == "reset": clear node state → return Ok(no exec output).
3. Read current state from `workflow_node_state` where `(graph_id, node_id, key="tg")`.
4. **If idle (or state missing)**:
   - Write `{ state: "armed", expires_at: now + wait_secs }` to node state.
   - Enter polling loop (250ms tick):
     - Re-read state from DB.
     - If state reset to idle by another run → exit (no output).
     - If `now >= expires_at` → break.
   - Write state = idle.
   - Return exec output on "out" port.
5. **If armed** (another run is polling):
   - Compute `new_expires = max(expires_at, now + extend_secs)`.
   - If `new_expires > expires_at`: update DB.
   - Return immediately with NO exec output (the keeper run will fire out when ready).

### Concurrency safety

Use `UPDATE workflow_node_state SET value = $1 WHERE graph_id=$2 AND node_id=$3 AND key=$4`
— no SELECT FOR UPDATE needed since individual JSON updates are idempotent and the only
contention is `expires_at` extension (which is a monotone max operation).

---

## Web UI — node inspector

Node inspector panel (`sentinel-app/src/lib/components/sensor-field.svelte` or node props panel):
- **Wait (seconds)** — number input for `wait_secs`.
- **Extend by (seconds)** — number input for `extend_secs` (label: "Min remaining on re-trigger").
- Live state badge on the node body: `IDLE` / `ARMED – 7s left` (from node state SSE).

The live state display reuses the `node_body_live_state` pattern already planned
([[node-body-live-state]]).

---

## Typical graph

```
OnEvent("motion") ─→ StartRecording(cam)
                  └→ TimeGate(wait=30s, extend=10s).arm
                              └──── out ────→ StopRecording(cam)
```

- First motion: StartRecording fires, TimeGate arms for 30s.
- Second motion at T+25s: TimeGate extends (25s remaining > 10s → no extension needed).
- Third motion at T+48s (3s remaining): TimeGate extends (3s < 10s → extend to 10s remaining).
- At ~T+58s: TimeGate fires → StopRecording.

---

## Out of scope (v1)

- Named gate instances (for multi-camera parallel gates — use separate graph copies per camera).
- Configurable polling interval (250ms is fine).
- `expires_at` countdown shown in workflow runs log (could be a follow-up).
