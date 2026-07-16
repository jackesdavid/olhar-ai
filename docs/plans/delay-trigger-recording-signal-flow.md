# Delay Trigger node — decouple Start/Stop Recording via a leading+trailing debounce

**Board card:** #86 Delay Trigger node — decouple recording start/stop (In Progress)
**Status:** built. Corrections found at implementation time are recorded in *Built — deltas from
this plan* at the bottom; read that before trusting the phase details above.
**Companion:** [[segmented-continuous-recording]] (#87 — caps clip size for long sessions)
**Goal:** One clean node that owns the leading+trailing edge so `StartRecording` becomes a plain,
stateless action called exactly once per clip.

## Context — why this change

Motion-triggered recording is wired today as:

```
OnEvent("motion") → StartRecording → TimeGate → StopRecording
```

Every motion event fires a **fresh graph run**, so `StartRecording` is called on *every* event. It
only works because idempotency is enforced on the **edge** (`sentinel-edge/src/recording.rs`
`RecordingManager::start()` scans active sessions by `camera_ip` and returns the existing `clip_id`
with `new:false` if already recording). The `TimeGate` is a trailing-edge debounce that holds a keeper
thread until quiet, then fires `StopRecording`.

This coupling is obscure: `StartRecording` depends on hidden edge-side dedup, which removed flexibility
(per-trigger clips, clean multi-trigger flows). **The fix moves the leading/trailing edge logic into one
node** (a UE **Retriggerable Delay** with an added leading pulse):

```
OnEvent("motion") → DelayTrigger
                       ├─ start ─→ StartRecording
                       └─ stop  ─→ StopRecording
```

- `start` (exec out) fires on the **first** signal of an idle window (leading edge).
- `stop` (exec out) fires once `wait_secs` elapse with **no further signal** (trailing edge); each new
  signal extends the window (debounce).
- Config: `wait_secs` (initial window) + `extend_secs` (top-up on re-trigger), same as Time Gate.

## The core engine constraint (why the trailing edge is special)

The scheduler (`engine.rs` `forward_epoch`) is a **synchronous multi-epoch Kahn pass**. A node's inline
handler runs to completion *before* the pass settles it and lets downstream-of-its-lit-pins run. So a
single upstream node **cannot block** to wait — if it did, `StartRecording` (downstream of `start`)
would be delayed until after the wait. Time Gate gets away with a thread-holding keeper only because
`StopRecording` is downstream *of it*, with nothing that must fire first.

**Therefore:** `start` fires synchronously in the triggering run; `stop` must be delivered by a
**separate, later run forced at the DelayTrigger node**. Schedule that later run with the durable
`work_jobs` queue (migration 0051's `run_after`), not a live thread — crash-safe, no new engine plumbing.

**Distinguishing the two runs in one handler:** branch on `forced.contains(&nid)` (already in scope in
`forward_epoch`). In a normal run the *OnEvent* node is the forced entry and DelayTrigger runs only
because a live exec pin feeds it → **trigger** path (arm / extend). In the trailing sub-run, DelayTrigger
*itself* is the forced entry → **expiry** path (light `stop`). No sentinel flag needed.

## Cycle semantics — re-arming after stop

The node is a one-bit state machine (`armed` | `idle`) and **cycles**:
- First input on an `idle` node → fire `start`, flip to `armed`, schedule the stop timer.
- Input while `armed` → extend `expires_at` only (no `start`).
- Quiet for the window → fire `stop`, flip to `idle`.
- **The next input after `stop` → node is `idle` again → fire `start` again = a brand-new window.**

So each *burst* of input is one complete `start → stop` cycle = one recording session. Sustained input
keeps extending one window (one session, internally chunked by [[segmented-continuous-recording]]); a gap
≥ `wait_secs` closes it and the next input opens a fresh session. This is the intended "one clip per
motion burst" behavior.

**Atomicity (exactly one start / one stop per cycle):** the `idle ↔ armed` transition must be atomic so a
new input arriving at the same instant the stop timer fires cannot double-fire or drop an edge. Guard the
node_state row with `SELECT … FOR UPDATE` + a conditional `UPDATE` in a transaction (compare-and-swap on
the `state` column — never `UPDATE … RETURNING` to read the prior value; repo rule
`feedback_postgres_returning`):
- **Trigger run:** CAS `idle → armed`; if it wins, fire `start` + enqueue timer; if it loses (already
  `armed`), take the extend path (no `start`).
- **Stop consumer:** fire `stop` only if it CAS-flips `armed → idle` **and** `now >= expires_at`; if an
  input extended `expires_at` first, it instead reschedules the job (no `stop`).

## Resolved decisions

- **Name:** label `Delay Trigger`, slug `delay_trigger`.
- **Category:** `Control` (with Time Gate / Cooldown / FlipFlop). Already has frontend color+icon → **no
  frontend change**.
- **Timing:** `wait_secs` (default 10) + `extend_secs` (default 10) — parity with Time Gate.
- **Timer mechanism:** durable `work_jobs` (`run_after`) + a poll consumer. Crash-safe; ≤~500 ms poll
  latency on `stop` (irrelevant for recording).
- **Edge idempotency:** **keep** the `camera_ip` guard in `recording.rs` as defense-in-depth. Any removal
  is a separate follow-up card.
- **Frontend:** none. Catalog is schema-driven — two exec outs `start`/`stop` auto-render as stacked pins
  in the node body; `wait_secs`/`extend_secs` auto-render as config inputs.

## Implementation — phased, file-level

### Phase A — Node type surface · `crates/app-core/src/workflows/domain.rs`
Add arms next to the existing `TimeGate` entries:
1. Enum variant `DelayTrigger,` (Control cluster, ~line 435).
2. Exec-out consts beside `EXEC_OUT_TIME_GATE` (~818):
   `const EXEC_OUT_DELAY_TRIGGER: [Port; 2] = [p("start", PortType::Exec, false), p("stop", PortType::Exec, false)];`
3. `NodeKind::ALL` (~837) — add the variant; bump array length (`[NodeKind; 64]` → `65`). **Update the
   "61 user-visible nodes" count** referenced in `CLAUDE.md` / node docs.
4. `kind_str`/slug (~939–965) → `"delay_trigger"`.
5. `label` (~1013–1039) → `"Delay Trigger"`.
6. `category` (~1065–1072) → `"Control"`.
7. `exec_out_ports` (~1147–1152) → `&EXEC_OUT_DELAY_TRIGGER`.
8. `input_ports` (~1213) → `&NO_PORTS` (exec-only; timing is config, like Time Gate).
9. `output_ports` (~1278) → `&NO_PORTS`.
10. `default_config` (~1384) → `json!({ "wait_secs": 10, "extend_secs": 10 })`.

### Phase B — Dispatch stubs · `crates/app-core/src/workflows/node.rs`
- `run_node` arm → return empty `PortValues` (engine-driven, like `TimeGate`/`Cooldown`).
- `node_timeout` arm → `None` (handler never blocks).
- `debug_summary` arm → short line ("Delay Trigger: armed for Ns" / "extended +Ns" / "window closed → stop").

### Phase C — Engine handler · `crates/app-core/src/workflows/engine.rs`
New inline block in `forward_epoch`, right after the `TimeGate` block (after ~line 719). Reuse the
TimeGate config-parse + `repo::get_node_state`/`set_node_state` idioms and the `active_gates` key (~598).
Ends with `continue`, sets its own `live_out`. **No `thread::sleep` — returns immediately in every branch.**

- `is_forced = forced.contains(&nid)`.
- **Forced (expiry delivery):** `live_out.insert((nid, "stop".into()))`; set node_state `idle` (or
  `repo::clear_node_state`); remove `active_gates` key; debug "window closed → stop".
- **Trigger (fed live):** read state; `is_armed = state.state == "armed"`.
  - **Idle → arm (leading edge):** `live_out.insert((nid, "start".into()))`; set node_state
    `{ state:"armed", expires_at: now+wait_secs }`; `active_gates.insert(key)`; enqueue the durable stop
    timer (`enqueue_window_stop_blocking`, Phase E) with `run_after = expires_at`.
  - **Armed → extend:** if `remaining < extend_secs`, bump `expires_at = now+extend_secs` in node_state;
    light **nothing**. Do not touch the job — the consumer reschedules by re-reading `expires_at`.
- Wrap the idle↔armed read+write in the CAS transaction from *Cycle semantics* above.

### Phase D — State helper · `crates/app-core/src/workflows/repo.rs`
Add `clear_node_state(db, tenant, graph, node)` (a `DELETE`) so an idle window leaves no stale row. (Also
unblocks the pending Cooldown `reset` card #13.) Optional — `set_node_state({state:"idle"})` matches Time
Gate if we skip it. Add the `SELECT … FOR UPDATE` + conditional-update helper for the CAS transitions.

### Phase E — Durable timer · `crates/app-core/src/work_queue.rs`
- Constants: `WINDOW_CATEGORY = "workflow_window"`, `WINDOW_KIND = "stop"`.
- `enqueue_window_stop_blocking(db, tenant, graph, node, run_after_rfc3339)` — like the existing
  `enqueue_one_blocking` but binds an explicit `run_after` (current path hard-codes `run_after =
  created_at` at ~line 63). Payload `{ graph, node, expires_at }`.
- `reschedule_blocking(db, id, run_after_rfc3339)` — sets `status='pending', run_after=$2` for a claimed
  job (extend / not-yet-due). Sibling of `retry_after_blocking` but with an explicit target time.
- **No migration** — `work_jobs` already carries arbitrary `category`/`kind`/`payload`/`run_after`
  (migrations 0050 + 0051).

### Phase F — Host consumer + stop sub-run · `sentinel-flow/`
1. **New `sentinel-flow/src/window_consumer.rs`** — clone of `event_consumer.rs`:
   - Poll `work_queue::claim_blocking(db, WINDOW_CATEGORY, WINDOW_KIND, BATCH)`.
   - `process(state, job)`: read `graph`,`node`; re-read `repo::get_node_state`:
     - not `armed` → `mark_done_blocking` (window reset/closed), no-op.
     - `armed && now >= expires_at` → spawn the stop sub-run, then `mark_done_blocking`.
     - `armed && now < expires_at` (extended) → `reschedule_blocking(db, job.id, expires_at)`.
2. **`spawn_node_subrun` in `sentinel-flow/src/engine.rs`** — mirror `spawn_event_handler` (~535) but force
   the DelayTrigger node: create a run record, call `execute_run(state, tenant, &graph, run_id,
   Some(node_id), payload)`. `execute_run` already routes `Some(entry)` → `run_subpath_eventful`
   (`seed_entries=false, initial_forced={node_id}`, ~306–315), so only DelayTrigger + its `stop`
   descendants run. Give it a distinct trigger label (new `TriggerSource::Timer`, or `Event` emitter
   "delay") for clean run history.
3. **Register at boot** — add `window_consumer::spawn(state.clone());` next to `event_consumer::spawn(...)`
   in `sentinel-flow/src/main.rs`. `active_gates` is already shared via `AppState`.

### Phase G — Docs · `docs/workflow-nodes.md`
Add a catalog row next to Cooldown / Time Gate (hand-maintained doc):
`| **Delay Trigger** | — | — | wait_secs (10), extend_secs (10) | Leading+trailing debounce (UE Retriggerable Delay + start pulse). start fires on the first signal of an idle window; stop fires once wait_secs elapse with no further signal (each signal extends). start runs exactly once per window. | OnEvent(motion) → Delay Trigger; start → Start Recording; stop → Stop Recording |`

## Crash / restart recovery (free with work_jobs)
Armed state lives durably in **two** places: `workflow_node_state` (`state:"armed", expires_at`) and the
`work_jobs` stop row (`run_after = expires_at`) — both survive a pod restart. On boot, `window_consumer`
resumes polling and fires any due `stop` (immediately if `expires_at` already passed during downtime, else
on schedule). A `running` stop job whose sub-run crashed is reaped by `claim()`'s staleness reap (0051) and
retried — a double-stop is safe (edge stop of an already-stopped clip is a no-op at `recording.rs:408`).

## Verification
1. **Engine unit test** (add to the `#[cfg(test)]` block in `engine.rs`, following the
   `run_subpath_eventful` / FlipFlop / Cooldown tests):
   - Graph `OnEvent(motion) → DelayTrigger`, `start → probeA`, `stop → probeB`.
   - Fire a normal trigger → assert `start` fired (probeA ran), `stop` did not, node_state `armed` with an
     `expires_at`, and a `work_jobs` row exists for `WINDOW_CATEGORY`.
   - Fire a second trigger before expiry → assert no new `start`, `expires_at` extended.
   - Force a run at the DelayTrigger node (simulated expiry) → assert `stop` fired (probeB ran), state idle.
   - After the forced stop, fire another trigger → assert `start` fires again (new cycle).
2. **End-to-end** (`task flow` / `cargo run -p sentinel-flow`, which now also spawns `window_consumer`):
   inject 2–3 `motion` events for a camera within `wait_secs`; watch the workflow-log SSE + run history —
   exactly **one** StartRecording at the first event, and **one** StopRecording ~`wait_secs` after the last,
   delivered as a *separate* run whose trigger reads as the window timer. Confirm the `workflow_window/stop`
   job goes `pending → running → done`.
3. **Restart test:** arm a window, restart the flow pod before expiry, confirm `stop` still fires on the
   consumer's first post-boot poll.
4. **Node debug:** enable `graph.debug` and read the `debug_summary` lines to trace the state machine.

## Out of scope (v1)
- Removing the edge-side `camera_ip` idempotency guard (separate follow-up card).
- A `reset` exec input on Delay Trigger (can mirror the Cooldown `reset` card #13 later).
- Migrating existing saved graphs off the old `Start → TimeGate → Stop` pattern (both remain valid).
- Exposing elapsed/remaining time as a wirable data output.
- **Clip segmentation** — see companion [[segmented-continuous-recording]] (#87).

---

## Built — deltas from this plan

The plan was written from an earlier read of the code. What actually shipped, and why it differs:

- **`kind_str` doesn't exist** — the slug accessor is `NodeKind::slug()`. `from_slug` derives from
  `ALL` (`ALL.into_iter().find(|k| k.slug() == s)`), so `slug_round_trips_all_kinds` covers the new
  node automatically — no test edit, and no catch-all arm to miss.
- **`NodeKind::ALL` 64 → 65** as planned. `CLAUDE.md` said "61 user-visible nodes" — already stale
  by 3 before this card; corrected to 65.
- **`work_queue` could not schedule anything.** `enqueue()` hard-codes `run_after = created_at`, so
  "enqueue with a future `run_after`" needed building, not calling: added `enqueue_at` /
  `enqueue_at_blocking` (no `NOTIFY` — waking a consumer for a job that isn't due only burns a poll),
  `reschedule` / `reschedule_blocking`, the `WINDOW_CATEGORY`/`WINDOW_KIND` pair, and
  `enqueue_window_stop_blocking`. `claim` already gates on `run_after <= now`, so it needed no change.
- **Atomicity: conditional writes, not `SELECT … FOR UPDATE`.** `workflow_node_state` had no CAS of
  any kind. Rather than the planned lock-then-update transaction, each transition is a single
  conditional statement whose `rows_affected` says who won — simpler, atomic, and it never reads an
  old value (so the `feedback_postgres_returning` rule doesn't even apply):
  - `try_arm_node_state` — `INSERT … ON CONFLICT DO UPDATE … WHERE state IS DISTINCT FROM 'armed'`
  - `extend_node_state` — `UPDATE … WHERE state = 'armed'`
  - `try_disarm_expired_node_state` — `UPDATE … WHERE state = 'armed' AND expires_at::timestamptz <= now()`
  Postgres compares against its **own** `now()`, so the decision is atomic with the write and immune
  to clock skew between pods. (`state_json` is `TEXT`, hence the `::jsonb` cast.)
- **`forced` alone is not a safe discriminator.** A loop back-edge also forces a node, which would
  fake an expiry and fire a spurious `stop`. The expiry run is instead marked by
  `WINDOW_STOP_MARKER` in the entry payload — inert for every other node, since only `OnEvent`
  consumes `entry_payload`.
- **The consumer owns the disarm.** `window_consumer` wins (or loses) `try_disarm_expired_node_state`
  and only then forces the run; the engine block trusts that and just lights `stop`. On a loss it
  re-reads the state: still armed → `reschedule_blocking` to the new expiry; idle → the cycle is over,
  `mark_done_blocking`.
- **`TriggerSource::Timer`** added (the enum had only Manual/Cron/Event), so a trailing stop reads as
  its own trigger in run history rather than masquerading as an event.
- **No engine unit test.** The plan assumed it would follow "the FlipFlop / Cooldown tests" — those
  don't exist. Every stateful engine node is DB-driven, and the suite passes without Postgres only
  because `get_node_state` swallows errors to `None`. A DB-backed test would break `cargo test` for
  anyone without a database. The CAS semantics — the actual risk — were verified directly against
  Postgres instead (all 8 cases: first arm wins, re-arm loses, early disarm loses, extend wins,
  expired disarm wins, double disarm loses, extend-while-idle loses, re-arm after stop wins).

## Still to verify at runtime

The end-to-end path (motion burst → one `start`, quiet → one `stop`, restart mid-window still stops)
has **not** been driven yet — it needs flow + Postgres + a graph firing real events.
