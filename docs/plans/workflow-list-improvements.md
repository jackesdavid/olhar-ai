# Workflow List Improvements — Implementation Plan

**Board card:** #18 Workflow list improvements (Done)
**Goal:** Make the workflow list immediately actionable — search, last-run at a glance, a description, and a duplicate action.

## Shipped (2026-07-01)
Delivered as a full-width Recordings-style table, and adapted to the post-UUID data model
(graph ids are `Uuid`; persistence is the JSON-blob `workflows` table, not a columnar
`workflow_graphs` table — so `description`/`group` are `#[serde(default)]` fields, no migration):
- **Last run** — `repo::last_run_by_graph` (`DISTINCT ON (graph_id)`), surfaced in a combined
  Status·Last-run column with a `timeAgo()` helper.
- **Description** — `Graph.description`, `PUT /workflows/:id/description`, shown as a caption
  under the name + editable via `<Textarea>` in Configuration.
- **Search / filter** — implemented **backend** (`GET /workflows?q=&group=`), not client-side.
- **Duplicate** — `POST /workflows/:id/duplicate`.
- **Groups / labels** (was out of scope below) — `Graph.group`, `PUT /workflows/:id/group`,
  a group filter + rows clustered into section headers like the logs view.

## Phase 1 — Last run summary in list

The `workflow_runs` table already has `started_at`, `status`, `trigger_source` per graph.
A single LEFT JOIN aggregation per `graph_id` surfaces the most recent run.

- `crates/app-core/src/workflows/repo.rs`: add `list_graphs_with_last_run()` — wraps
  `list_graphs()` with a batch query:
  ```sql
  SELECT DISTINCT ON (graph_id) graph_id, started_at, status, trigger_source
  FROM workflow_runs WHERE tenant_id = $1
  ORDER BY graph_id, started_at DESC
  ```
  Returns `HashMap<u32, LastRun>` merged into the response.
- `crates/app-core/src/workflows/domain.rs`: add `last_run: Option<LastRun>` to `Graph`
  (`{ at: String, ok: bool, trigger: String }`).
- `sentinel-control/src/domain/workflows.rs` `list()`: use the new combined query.
- Frontend `+page.svelte`: replace the Status column with a combined **Status / Last run**
  column — shows running/idle/disabled badge + "3 min ago ✓" or "2h ago ✗" below it.
  Use a `timeAgo(at)` helper (existing pattern in the codebase). No extra API call.

## Phase 2 — Description field

- Migration `0023_workflow_description.sql`:
  `ALTER TABLE workflow_graphs ADD COLUMN description text NOT NULL DEFAULT ''`
- `Graph` struct gains `description: String` (`#[serde(default)]`).
- `sentinel-control/src/domain/workflows.rs`: add `PATCH /workflows/:id/description`
  (same pattern as `rename` / `set_trigger`).
- Frontend: show description as a second line under the workflow name in the list
  (muted, clamped to 1 line). Editable in the Configuration side panel with a
  `<Textarea>` — auto-saves on blur (debounced, same pattern as node config).

## Phase 3 — Search / filter

Pure frontend — no backend change.

- Add a `filterQuery = $state("")` and a `<Input placeholder="Filter…">` above the table.
- Derive `visibleGraphs` from `graphs.filter(g => g.name.toLowerCase().includes(q))`.
- Clear button (×) when query is non-empty. Matches on name + description.

## Phase 4 — Duplicate workflow

- `sentinel-control/src/domain/workflows.rs`: `POST /workflows/:id/duplicate` —
  loads the source graph, deep-copies nodes/edges/trigger/concurrency/config, names it
  `"<name> (copy)"`, inserts, returns the new id. No migration.
- `crates/app-core/src/workflows/repo.rs`: `duplicate_graph(db, tenant_id, source_id)` —
  fetch the JSON blob, strip the id, insert fresh.
- Frontend: add a **Duplicate** button in the Actions column (copy icon, ghost/icon variant,
  same row as Run/History/Delete). On success, reload the list and open the new graph.

## Out of scope (v1)
- Sort by column header (nice to have, client-side)
- Bulk enable/disable
- Pagination (handle in a future scale pass)
