# Dictionaries — named typed-column tables, mutable & thread-safe through graphs

**Board card:** #113 Dictionaries — reusable typed tables + nodes (In Progress)
**Goal:** A **Dictionary** is a named, tenant-scoped, **typed-column table** (columns-with-types =
structure; data rows = typed values). It's a first-class reusable entity: workflow variables
**reference** one by id (edit once → every workflow sees it), a **dedicated view** manages them
(create/edit/remove), a **big-modal** table editor can create one live from a variable row, and
**workflow nodes** read and **mutate** them mid-run. Mutations are **atomic + thread-safe** across
concurrent runs (Postgres single-row lock).

Supersedes #102 Phase 3 ("typed-column Dictionary table"), which this absorbs and extends with the
reference model + mutation nodes + thread-safety.

## The shape (decided with the owner, 2026-07-17)

- **Structure = typed columns.** Each column has a `name` + a cell type — start **Text / Num / Bool**.
- **Rows = typed values**, one cell per column.
- **Reference model** (not copy): a Dictionary variable's default stores the **dictionary id**;
  editing the dictionary in the dedicated view propagates to every workflow referencing it. Delete is
  guarded while in use.
- **Naming/ports:** keep the existing flat `PortType::Dictionary` (string→string) for HTTP Post
  `headers`/`fields` — untouched. Introduce a NEW `PortType::Table` + `PortValue::Table` for the
  structured thing. The user-facing word is "Dictionary"; the variable **Dictionary** type maps to
  `PortType::Table` (a reference to a named dictionary). Flat dictionaries stay node-config-only.
- **Thread-safety:** each mutation runs in a transaction that `SELECT … FOR UPDATE`s the dictionary's
  single row before rewriting its `rows` JSON — serializing all concurrent mutations to that
  dictionary across runs/pods.

## Grounding (real files)

- `PortValue` / `PortType` — `crates/app-core/src/workflows/node.rs` (PortValue enum ~L40) &
  `domain.rs:33` (PortType). Flat dict is `PortValue::Dict(Vec<(String,String)>)`.
- `parse_variable_value` (`node.rs:319`) — the single seed/override parse point; `_ => None` today for
  Dictionary. Table resolves an id → `PortValue::Table` from `sys` (needs a dictionaries map on ctx).
- `ToolContext` (`agent/tools.rs:77`) — has `devices`, `variables`, `db`, `tenant_id`. Add a loaded
  `dictionaries` snapshot for run-start reads; mutations write through `db`.
- `NodeKind::ALL` (`domain.rs:856`, currently **65**) + execution in `node.rs`; catalog auto-derives
  (`docs/workflow-nodes.md`). `agent/tools.rs` does NOT list workflow nodes (no change there).
- Variable-type dropdown `+page.svelte:117-127` (has "dictionary"); `VariablesPanel.svelte`
  `DEFAULTABLE` set (L45) excludes dictionary — the gap this fixes.
- Control CRUD route pattern: `sentinel-control/src/domain/workflows.rs` (templates/global endpoints
  are the mirror). Frontend data layer `sentinel-app/src/lib/data.ts`.
- Dedicated-view precedent: `/workflows/templates/+page.svelte` (PageHeader/EmptyState/Loader).
- Next migration: **0059** (CLAUDE.md's "0055" is stale; files run to 0058).

## Phase 1 — Storage + the atomic table (backend, no UI)

- **Migration 0059** `dictionaries`: `id uuid pk`, `tenant_id uuid`, `name text`, `columns jsonb`
  (`[{name, type}]`), `rows jsonb` (`[[cell,…]]`), `created_at text`, `updated_at text` (RFC3339 text —
  sqlx has no timestamptz). Index `(tenant_id)`; unique `(tenant_id, lower(name))`.
- **Domain** (`app-core`): `Dictionary { id, name, columns: Vec<DictColumn>, rows: Vec<Vec<DictCell>> }`,
  `DictColumn { name, ty: CellType }`, `CellType = Text|Num|Bool`, `DictCell` = typed scalar. Serde to
  the jsonb shape.
- **Repo** (`app-core`, new `dictionaries.rs` or in workflows repo): `list(tenant)`,
  `get(tenant,id)`, `create`, `rename`, `set_columns`, `delete` (guarded), and the **atomic mutators**
  used by nodes — `add_row`, `update_row`, `delete_row`, `set_cell`, `clear_rows` — each in a tx that
  `SELECT … FOR UPDATE`s the row first. `usage_count(tenant,id)` scans workflows' variables for refs.
- **`PortType::Table` + `PortValue::Table { columns, rows }`**; `parse_variable_value` Table arm
  resolves the id from a `sys.dictionaries` snapshot.

## Phase 2 — Control CRUD + data layer

- Control routes (tenant-scoped, mirror templates): `GET/POST /dictionaries`, `GET/PUT/DELETE
  /dictionaries/:id`, `PUT /dictionaries/:id/columns`, row mutation endpoints (also reused by nodes
  internally via repo, not HTTP). Delete → 409 while `usage_count > 0` with the referencing workflow
  names.
- `data.ts`: `listDictionaries`, `getDictionary`, `createDictionary`, `updateDictionary`
  (name/columns), `deleteDictionary`, row ops for the editor.

## Phase 3 — Dedicated view + big-modal table editor (frontend)

- New route `/workflows/dictionaries/+page.svelte` — impeccable list (usage count, Global-style
  badges N/A), create/edit/remove. PageHeader/EmptyState/Loader; `.interactive` cards.
- New `table-editor.svelte` — add/remove/rename/retype **columns** (type select per column);
  add/remove/edit **rows** with per-column typed cell inputs (Text `<input>`, Num `number`, Bool
  `switch`). Used both in the view and inside the big modal.
- **Variable reference editor** in `VariablesPanel.svelte`: for a Dictionary (Table) variable, a
  `<select>` of the tenant's dictionaries + a **"＋ New / Edit"** button opening a **large** Dialog
  hosting `table-editor`, create live → set the variable's default to the new id. Add `table`/
  `dictionary` to the defaultable handling.

## Phase 4 — Dictionary nodes (read + mutate, atomic)

Add to `NodeKind::ALL` + execution in `node.rs`; catalog auto-derives. A `Dictionary`/`Table` input
port selects the target (from a Table variable or a literal ref).
- **Read:** `DictGetRow` (at index), `DictFindRow` (first row where column == key), `DictGetCell`
  (row,col), `DictRowCount`, maybe `DictGetColumn` (a column as a List).
- **Mutate (atomic):** `DictAddRow`, `DictUpdateRow` (by index or matched key), `DictDeleteRow`,
  `DictSetCell`, `DictClear`. Each calls a repo mutator (row-locked tx) → safe under concurrency.
- Update `docs/workflow-nodes.md`. No `agent/tools.rs` change (workflow nodes auto-derive).

## Open sub-decisions (decide at build)

1. **Cell JSON encoding** — store cells as tagged (`{t:"num",v:3}`) or positional typed-by-column
   (rows are `[val,…]`, types come from `columns`). Lean: positional, types from columns (compact).
2. **Row identity for mutate-by-key** — match on a chosen key column's value; first match wins
   (mirrors `GetFirstByKey`). Multi-match + "update all" is out of v1.
3. **Table → node port runtime** — a Table variable resolves to `PortValue::Table` at run start;
   mutation nodes re-read/lock live from the DB (not the run-start snapshot) so cross-run writes are
   seen. Confirm snapshot-for-reads vs always-live: **mutators always hit the DB row (locked); pure
   reads may use the snapshot** — document the consistency contract.
4. **Bounds** — max rows/cols per dictionary; cell size caps (guard the jsonb from unbounded growth).

## Out of scope (v1)

- Cell types beyond Text/Num/Bool (Date, Camera, Recipient cells later).
- Column constraints / validation / uniqueness / required.
- Cross-tenant / global shared dictionaries (templates handle sharing separately).
- Query nodes beyond first-match (filter/sort/aggregate) and multi-row transactions across dicts.
- Migrating HTTP Post's flat `headers`/`fields` to Table (they stay flat `Dict`).
