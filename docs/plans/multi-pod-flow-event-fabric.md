# Multi-pod sentinel-flow: Shared Event Fabric — Implementation Plan

**Board card:** Multi-pod sentinel-flow: shared event fabric for SSE + activity (Backlog)
**Goal:** Let `sentinel-flow` scale to N replicas without the UI losing live run events — today the event bus + run-activity map are per-pod and in-memory, so the control relay only sees the one pod it connected to.

## Context

Today flow is a **single** multi-tenant pod. Two things are pod-local:

1. **The SSE event bus** — a per-pod `tokio::broadcast` channel (`sentinel-flow/src/events.rs`). Each pod only broadcasts its own runs.
2. **The run-activity map** — `Activity = Arc<Mutex<HashMap<(tenant, graph_id), GraphActivity>>>` in `sentinel-flow/src/state.rs`, in-memory.

The control relay opens **one** connection to `FLOW_URL/events` (`sentinel-control/src/domain/workflows.rs`), so with N replicas behind a load balancer it would only ever stream the runs that happen to execute on the pod it landed on. This affects **all** SSE events — `workflow-run` / `-node` / `-log` / `-activity` — equally. (The old 700 ms `GET /runs` poll had the same single-pod blindness — see [[workflow-runs-realtime]].)

## Phases

### Phase 1 — Shared event bus (pub-sub)
- Replace (or fan out from) the per-pod `tokio::broadcast` with a shared pub-sub fabric — Redis pub-sub or NATS — so any pod's run events reach every subscriber.
- The control relay subscribes to the shared channel instead of one pod's `/events`.

### Phase 2 — Shared / queryable activity state
- Move the in-memory `GraphActivity` map to shared state (e.g. Redis hash keyed by `(tenant, graph_id)`), or make the relay's REST seed (`GET /runs`) fan in across replicas.

### Phase 3 — Relay fan-in
- Control's `events_stream` either subscribes to the shared bus (Phase 1) or fans in from all replicas; keep the existing tenant-isolation filter on the merged stream.

## Open decisions
1. Redis vs NATS for the bus (Redis likely already in the stack; NATS is the cleaner fit for fan-out).
2. Shared activity store vs. derive activity purely from the shared event stream (no separate map).

## Out of scope
- Doing any of this now — flow is single-pod and correct as-is. This card is the plan for **when** flow needs to scale horizontally. Surfaced while doing [[workflow-runs-realtime]].
