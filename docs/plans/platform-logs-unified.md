# Platform Logs — Unified Audit Log System

**Board card:** Unified Platform Logs (ToDo)
**Goal:** One `platform_logs` table and one log UI for every module in Sentinel, replacing the separate `box_logs` + `workflow_run_logs` silos with a single, paginated, deep-linkable, 30-day-TTL audit stream.

---

## Context — what exists today

| System | Table | Viewer |
|---|---|---|
| Workflow execution trace | `workflow_run_logs` (bigserial + run_id FK) | Workflows page drawer |
| Edge/box stdout | `box_logs` (bigserial + box_id FK, capped 5 000) | `/logs` page SSE tail |
| Run registry | `workflow_runs` | Workflows page drawer (kept as-is) |

Pain: two tables, two APIs, two viewers, no cross-module linkage, no tenant-wide audit, no TTL.

---

## Phase 1 — Schema (migration 0023)

Create `crates/app-core/migrations/0023_platform_logs.sql`:

```sql
CREATE TABLE platform_logs (
  id          bigserial PRIMARY KEY,
  tenant_id   uuid    NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  module      text    NOT NULL,          -- 'workflow' | 'edge' | 'presence' | 'device' | 'auth' | 'alert'
  trace_id    uuid,                      -- propagated from triggering parent; NULL = root event
  entity_id   text,                      -- e.g. graph_id cast to text, box_id, device_id
  entity_type text,                      -- 'workflow_graph' | 'edge_box' | 'device' | ...
  level       text    NOT NULL DEFAULT 'info',   -- 'debug' | 'info' | 'warn' | 'error'
  message     text    NOT NULL,
  payload     jsonb,                     -- module-specific structured data
  created_at  text    NOT NULL           -- RFC3339 set by app (now_rfc3339()), not timestamptz
);

-- Hot path: tenant × module × recency
CREATE INDEX platform_logs_tenant_module_idx ON platform_logs (tenant_id, module, id DESC);
-- Tenant-wide recency (dashboard, "all" view)
CREATE INDEX platform_logs_tenant_id_idx    ON platform_logs (tenant_id, id DESC);
-- Trace linkage across modules
CREATE INDEX platform_logs_trace_idx        ON platform_logs (trace_id) WHERE trace_id IS NOT NULL;
-- Entity drill-down (e.g. one graph's week of logs)
CREATE INDEX platform_logs_entity_idx       ON platform_logs (tenant_id, entity_type, entity_id, id DESC)
  WHERE entity_id IS NOT NULL;
```

**Notes:**
- `created_at text` — project rule: sqlx built without chrono/time, can't decode `timestamptz`. RFC3339 is lexicographically sortable so string comparison works for date filtering.
- `id DESC` index suffix makes newest-first queries index-only.
- Do NOT drop `workflow_run_logs` or `box_logs` yet — deprecate in place; remove in a later migration once UI is fully ported.

---

## Phase 2 — Core library (`crates/app-core/src/platform_logs.rs`)

New public module in `app-core`. Exposes:

```rust
pub struct PlatformLog {
    pub tenant_id: Uuid,
    pub module:      &'static str,
    pub trace_id:    Option<Uuid>,
    pub entity_id:   Option<String>,
    pub entity_type: Option<&'static str>,
    pub level:       &'static str,   // "debug" | "info" | "warn" | "error"
    pub message:     String,
    pub payload:     Option<serde_json::Value>,
}

pub struct LogQueue(mpsc::Sender<PlatformLog>);

impl LogQueue {
    /// Spawn background drain task; returns the queue handle.
    pub fn spawn(pool: PgPool) -> Self { ... }

    /// Fire-and-forget — silently drops if channel is full (expected under load).
    pub fn push(&self, log: PlatformLog) {
        let _ = self.0.try_send(log);
    }
}
```

The drain task (`async fn drain(rx, pool)`):
- Bounded channel: `1 000` entries (back-pressure beyond that → drop).
- Wakes on: `rx.recv()` OR a 500 ms ticker.
- Drains up to 200 entries at once into a single `INSERT … (VALUES …)` batch.
- Errors are swallowed (`tracing::warn!`) — log writes must never crash callers.

**TTL sweep** (add to drain task or a separate daily timer in sentinel-control):
```sql
DELETE FROM platform_logs WHERE created_at < $1  -- $1 = now minus 30 days in RFC3339
  AND tenant_id = $2  -- sweep per-tenant to avoid long table locks
```
Run once per day per tenant, or at startup if last sweep > 24 h ago.

---

## Phase 3 — Instrumentation

Wire `LogQueue` into each service's `AppState` (sentinel-control + sentinel-flow both hold a `PgPool` already).

### sentinel-flow (`sentinel-flow/src/`)
- `run_started` → `module: "workflow"`, `entity_type: "workflow_graph"`, `entity_id: graph_id.to_string()`, `trace_id: run_id`, `payload: { trigger_source, graph_name }`.
- `run_finished` → same entity + trace_id, `payload: { status, result_summary, duration_ms }`.
- Each `RunLogLine` flush → push to platform_logs with `module: "workflow"`, `trace_id: run_id`, `payload: { kind, node_id, label }`. (Keep inserting into `workflow_run_logs` too for now — the workflow editor still reads from there.)

**Trace-ID propagation rule:**  
Nodes/handlers that receive a `trace_id` from a parent event **must not generate a new one** — they log with the parent's trace_id. Only root events (manual trigger, cron, first-arrival webhook) generate a fresh trace_id (`Uuid::new_v4()`).

### sentinel-control — edge log relay (`sentinel-control/src/domain/boxes.rs`)
- `POST /config/logs` handler currently inserts into `box_logs`. Also push to `LogQueue`:
  - `module: "edge"`, `entity_type: "edge_box"`, `entity_id: box_id.to_string()`, `level: line.level`, `message: line.message`, `payload: { target, ts }`.
- Keep inserting into `box_logs` for now (old page still reads it). Remove `box_logs` writes once frontend is ported.

### sentinel-control — device events
- Camera discovered / updated / deleted → push `module: "device"`, `entity_type: "device"`, `entity_id: device_id`.
- Box enrollment / re-key → push `module: "device"`, `entity_type: "edge_box"`.

### sentinel-flow — RecordPresence node
- Each `RecordPresence` execution → push `module: "presence"`, `entity_type: "zone"`, `entity_id: room_id`, `trace_id: inherited_from_trigger_if_any`, `payload: { persons_count, zone_name }`.

### Auth events (future — not in v1 scope)
- Login / token refresh / permission change are useful but out of scope for the first cut.

---

## Phase 4 — API

Add to sentinel-control (already the central API for the frontend):

```
GET /platform-logs
  ?module=workflow|edge|presence|device|alert   (optional, multi-value: module=workflow&module=edge)
  ?trace_id=<uuid>          (optional, exact match)
  ?entity_type=<str>        (optional)
  ?entity_id=<str>          (optional)
  ?level=info|warn|error    (optional, multi-value)
  ?from=<RFC3339>           (optional, created_at >= from)
  ?to=<RFC3339>             (optional, created_at <= to)
  ?after=<id>               (cursor, id < after for older pages)
  ?limit=<n>                (default 50, max 200)
```

Returns:
```json
{
  "items": [ { "id": 99, "module": "workflow", "trace_id": "...", "entity_id": "42",
               "entity_type": "workflow_graph", "level": "info",
               "message": "Run finished: ok", "payload": {...}, "created_at": "2026-06-19T..." } ],
  "next_cursor": 45,    // id of the last item; null if no more pages
  "total_in_page": 50
}
```

Cursor is `id` (bigserial) — "give me entries with id < cursor" for the next page (DESC ordering). This is stable even as new rows insert at the top.

---

## Phase 5 — Frontend

### New `/logs` page (rewrite)

Replace the current box-log-only `/logs` page with a general-purpose platform log viewer.

**URL param ↔ filter binding** (all filters are URL-serialized for deep-linking):
- `?module=`, `?trace_id=`, `?entity_type=`, `?entity_id=`, `?level=`, `?from=`, `?to=`
- Changing a filter → `replaceState` (no history push for each keystroke)
- Landing with params pre-fills filters and fetches immediately

**Filter panel (sidebar or top bar):**
- Module multi-select chips: All / Workflow / Edge / Presence / Device / Alert
- Level multi-select: Debug / Info / Warn / Error
- Date range picker: presets (Last hour / 6 h / 24 h / 7 days / 30 days) + custom from/to
- Trace ID input (paste a UUID → shows cross-module causality chain)
- Entity search: entity_type + entity_id text inputs

**Log table:**
- Columns: Timestamp | Module chip | Level | Message | Entity | Trace ID (copy button)
- Infinite scroll / "Load more" (cursor-based)
- Clicking Trace ID → fills `?trace_id=` filter to show full causal chain
- Clicking Entity → fills `?entity_type=&entity_id=` filter

**Reusable `LogsTable` component** (`sentinel-app/src/lib/workflows/LogsTable.svelte` or `src/lib/components/LogsTable.svelte`):
- Accept `items`, `loading`, `onLoadMore` props
- Used on: `/logs` page AND workflow editor run-log drawer (replace the current bespoke table)

### Deep links from other modules

**Workflow editor (in `sentinel-app/src/routes/workflows/+page.svelte`):**
- Run history row → "View in Logs" button → navigates to `/logs?trace_id=<run_id>`
- Graph header → "Last 7 days" button → navigates to `/logs?entity_type=workflow_graph&entity_id=<graph_id>&from=<7daysago>&to=<now>`

**Boxes/Devices page:**
- Each box row → "View Edge Logs" → `/logs?module=edge&entity_type=edge_box&entity_id=<box_id>&from=<24hago>`

---

## Open decisions

1. **Keep `workflow_run_logs` in parallel or cut-over immediately?**  
   Recommended: write to BOTH during phase 3–5. The workflow editor console reads from `workflow_run_logs` (simpler query with node_id/kind). Cut `workflow_run_logs` writes only once the unified logs table powers the editor too.

2. **TTL sweep location?**  
   Option A: background tokio task in sentinel-control, once per day.  
   Option B: pg_cron on the VPS (no app-side code). Preferred if pg_cron is available; check on VPS.

3. **Presence logging granularity?**  
   Every RecordPresence tick (could be frequent) vs. only on change (entered/left). Every tick is verbose; on-change is actionable. Recommend on-change with a `payload.count` field.

4. **Drop `box_logs` in same migration?**  
   Recommend keeping it as fallback until the frontend is fully ported (single migration 0023 adds; later 0024 drops).

---

## Out of scope (v1)

- Auth/permission change audit log
- Log export (CSV/JSON download)
- Log alerting rules ("email me if error-level logs spike")
- SSE real-time streaming of the unified log (re-add once table is stable)
- Per-tenant log retention settings (everyone gets 30 days)
