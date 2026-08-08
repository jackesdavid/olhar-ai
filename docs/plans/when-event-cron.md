# When Event → Cron: a per-node schedule, with a live countdown

**Goal:** let a single graph hold **several** independent schedules by giving the existing
`When Event` node a "Cron" selector, and show, inside the node, how long until it next fires.

**Driving case:** one graph with two of these — one turning Tuya devices on, another turning them
off, at different times.

## What exists, and why that decides the design

Almost all of this is already built, one level up. The work is mostly *moving* it, not inventing it.

| Piece | Where | State |
|---|---|---|
| `Schedule` — hourly/daily/weekly/monthly/yearly + IANA tz | `workflows/domain.rs` | done |
| `next_occurrence(sched, from)` | `workflows/schedule.rs` | done, with tests |
| Durable firing on `work_jobs` + reconcile on reload | `sentinel-flow/schedule_consumer.rs` | done |
| `Trigger::Cron { every_secs }` — a plain interval | `engine.rs` poller | done |
| Event → graph entry routing | `OnEvent` nodes | done |

Two things are true and shape everything below:

1. **A schedule is a GRAPH property today.** `Trigger::Schedule(_)` and
   `workflow_cron_state (tenant_id, graph_id, last_fired_ms)` both assume one timer per graph. Two
   schedules in one graph is precisely what the current model cannot express.
2. **`OnEvent` is already the entry-point mechanism.** An event arrives, the graph fires, the
   matching `OnEvent` node is the entry. So a scheduled node does not need a new execution path —
   it needs something to EMIT on time, and the existing routing does the rest.

That second point is the whole design: **a cron node is a scheduled emitter plus the routing we
already have.** Nothing new in the engine's execution model.

## Design

### The selector

`When Event` keeps its shape and gains a source selector:

- **Event** (today) — a system or tenant event by name
- **Cron** — a `Schedule`, configured in the node

Reusing the node rather than adding a `CronTrigger` kind is deliberate, and it was the user's call.
It keeps one entry-point concept in the catalog instead of two that behave identically downstream,
and every graph reading "When Event" already means "this is where a run starts".

### Firing

Each cron-configured node gets its own schedule row, keyed by **(tenant, graph, node)** — the
`graph_id`-only key is what has to change. On fire, flow enqueues an internal event addressed to
that specific node, and the existing entry-point routing takes over.

Addressing the NODE, not the event name, matters: two cron nodes in one graph must not wake each
other. Firing "a cron event" and letting both entry points match would run the whole graph twice
per tick and make the two-device case behave at random.

### The countdown

`next_occurrence` already computes the answer. The node needs `next_fire_at` and renders a live
countdown from it.

**Computed server-side, rendered client-side.** The browser could evaluate the schedule itself, but
then two implementations of "when does this fire" would have to agree forever — and the one users
believe is the one on screen. One source of truth, and the client only formats it.

## Decided: the graph-level time triggers GO

The node REPLACES them rather than joining them. Two ways to schedule the same graph is the
confusing outcome, and it would have needed a rule nobody could remember about which wins.

That deletes the first two open questions outright — there is no dual state to migrate and no
precedence to define. What is left is a `Trigger` enum with `Manual` and nothing else time-based.

**The cost is one graph, and it is small.** Production holds 30 graphs:

| Trigger | Count | Enabled |
|---|---|---|
| `Manual` | 23 | — |
| `Cron { every_secs }` | 4 | **none** |
| `Schedule(...)` | 3 | **one** — "Turn On/Off Daily", America/Manaus |

Every interval-cron graph is already disabled, so removing `Trigger::Cron` breaks nothing that
runs. `Trigger::Schedule` has exactly one live user — and it is the same daily on/off automation
this card exists to rebuild as two nodes, so it is a conversion rather than a loss.

**Order matters:** build the node → convert "Turn On/Off Daily" → then delete the triggers. Deleting
first would leave a working automation dark for however long the node takes.

## Decided: a late fire runs only inside a grace window

Today's behaviour is unbounded catch-up, and it is accidental rather than chosen: the job sits in
`work_jobs` with a past `run_after`, so whenever flow returns the consumer claims it and fires —
an hour late, a day late, it does not care. For a daily on/off that means a device can switch at
breakfast because the box was down at bedtime.

So: fire if the delay is under the window, skip otherwise, and say which in the run log. **This
changes the existing graph's behaviour too**, not just new nodes — worth stating plainly rather
than letting it surprise someone after a long outage.

**Default 15 minutes.** Long enough to cover the realistic outage — a deploy, a restart, a pod
reschedule — and short enough that a Tuya action never lands in a visibly wrong part of the day.
Per-node, because "late is fine" for a report and "late is wrong" for a light.

## Open question

**Countdown while disabled.** A disabled graph will not fire; the node should say so rather than
count down to something that will not happen.

## Test shape

The two-Tuya-node case IS the test: one graph, two schedules, devices on and off at different
times. It fails visibly and immediately if the addressing is wrong — both nodes firing on one tick
would toggle a device twice and the state would be wrong, not just late.

Worth asserting in code, because it is the property that has no other guard:
**a cron node's fire wakes that node and no other.**

## Related
[[multi-pod-flow-event-fabric]] · [[tuya-integration]]
