# Workflow parameterization + variables-panel overhaul (#102)

## Context

A default workflow **template** can't hardcode the author's camera — it has to *ask*. So a graph
needs **input variables** the user fills in, and the variables panel that edits them needs to be
genuinely usable. Requested across 2026-07-17, repeatedly deferred, and never carded until the owner
noticed. This plan is one epic (owner chose "all together") covering the panel UX, input variables,
a typed-column Dictionary table, and a shared template catalog.

The reframing insight, validated in code: the variables system (#56) already does most of the work.
`VariableDecl { name, ty: PortType, default: Option<String> }` (domain.rs:1800) is stored on
`Graph.variables` (domain.rs:1795), and `seed_variable_defaults` (node.rs:295) seeds each default
into `sys.variables` on **every** run path (engine.rs:248). So a variable's `default` is already its
run-start value. "Input variables" therefore = a `param` flag + a good editor + a manual-run
override + templates that read the params. And **clone already exists** (`POST /workflows/:id/duplicate`,
workflows.rs:823).

## Decisions locked (owner, 2026-07-17)

1. Dictionary default editor → **build a typed-column table** (columns with types, typed cells, row
   CRUD), not the existing string→string editor.
2. Templates → **shared central catalog**, cloned into the tenant on "Use template" (reuse duplicate).
3. Ship as **one epic**, phases below.
4. (Recommended, not blocking) An input's value **is** its `default` — setting an input = editing the
   default; manual-run overrides are ephemeral per-launch. A user owns their clone, so there's no
   pristine-default to protect.

## What already exists — reuse, don't rebuild

- **Clone**: `duplicate` (control workflows.rs:823) deep-copies the whole Graph incl. variables →
  generalize into the template "use" endpoint.
- **Recipient/camera dropdowns + "default to first"**: `data.listBeacons()` (data.ts:257,
  `NotificationTarget{label,chat_id,enabled}`), `recipientOptions`/`cameraOptions`
  (+page.svelte:668-669), default-to-first precedent (+page.svelte:1628,683). VariablesPanel already
  takes a `cameras` prop (VariablesPanel.svelte:19,93-102) — recipients mirror it.
- **MinIO primitive**: `media::put/get/delete` (crates/app-core/src/media.rs:62/73/86) — reuse for
  the File upload; only the browser→server route is missing.
- **Dictionary base**: `dictionary-editor.svelte` (string→string CRUD) — the typed-column table is a
  new sibling component, but its save wiring is the model.
- **Optimistic-update pattern** for the action-delay: commit `aa3e3ae` + `liveSaveConfig`
  (+page.svelte:2235) — local state, mutate immediately, debounced background PUT, **no reload**.

## Phase 1 — Variables panel overhaul (UX + latency)

The panel today derives read-only from `graph.variables` and every add/edit/remove awaits
`setGraphVariables` **then** a full `reload()` (`GET /workflows`, whole list). That serial round-trip
is the perceived lag.

- **Instant actions (the delay fix).** Mirror `aa3e3ae`: hold a local `$state` variables array, mutate
  it synchronously on add/edit/remove, persist `data.setGraphVariables` in the background (debounced
  ~400ms for typed fields, like `liveSaveConfig`), roll back on error, and **drop `reload()`**.
  Files: +page.svelte `saveVariables`/`addVariable`/`editVariable`/`removeVariable` (1669-1736). Keep
  `editVariable`'s Get/Set node re-sync but make it background too.
- **Layout rebalance.** In VariablesPanel.svelte the name `<Input>` is `flex-1` (unbounded) while type
  is `w-[68px]`. Give the name a bounded width (e.g. `flex-[2] min-w-0` or a max-width), widen type to
  ~`w-[96px]`, and open the row gap slightly (`gap-1` → `gap-1.5`). Row at :54, name :66, type :72.
- **Typed default editors** (extend the `DEFAULTABLE` branch, VariablesPanel.svelte:80-113):
  - **Recipient** → a `<select>` of recipients, default first. Pass a new `recipients` prop parallel
    to `cameras` (fed `recipientOptions` at +page.svelte:4193).
  - **Camera** → already a dropdown; confirm default-to-first.
  - **File** → a file-picker that uploads to MinIO (Phase 1b) and stores the returned object key as
    the default; show the filename.

### Phase 1b — File upload endpoint (new)

No browser→MinIO route exists (the current File picker captures only a filename, +page.svelte:2253).
Build a small authed multipart route in **sentinel-control** that calls `app_core::media::put(bytes,
mime, ext)` and returns `{ key }`; the File editor stores the key. `read_file_val` (node.rs:159) reads
a local path today — a follow-up teaches the File port to fetch from MinIO by key (or the value store
#94 subsumes it). v1: upload + store the key; wiring the key into run-time file reads is the small
remaining piece, noted.

## Phase 2 — Input variables (the `param` flag + manual-run prompt)

- **Model**: add to `VariableDecl` (domain.rs:1800), both `#[serde(default)]` (back-compat, no
  migration): `param: bool`, `description: Option<String>`. Mirror in the frontend `Variable` type +
  `setGraphVariables` mapping (data.ts:330, +page.svelte:1671) — currently drops unknown fields.
- **Promote**: a toggle per row in VariablesPanel calling `onEdit(i, { param: !v.param })`; param rows
  get a visual accent + an optional description field. This IS "mark them true/false as inputs," the
  UE model.
- **Setup surface**: a filtered view of `variables.filter(v => v.param)` reusing the same typed
  editors — "setting an input" = editing its default. Shown as a Setup section in the editor and as
  the first-run panel after cloning a template.
- **Manual-run override** (the one real backend seam): thread launch-time values into the run.
  1. Extract the per-type parser out of `seed_variable_defaults` into
     `parse_variable_value(ty, raw, sys) -> Option<PortValue>` (node.rs:302-315) and reuse it both
     places.
  2. `execute_run` (engine.rs:237) gains an `input_overrides: PortValues` param; after the seed at
     engine.rs:248, insert overrides into `ctx.variables` (overrides win over defaults). Do **not**
     reuse `entry_payload` (that's the OnEvent-subtree channel).
  3. Thread through `fire_graph` (engine.rs:79) → `spawn_graph_run` (engine.rs:105) → the execute_run
     call at engine.rs:149. Event/timer subtrees (engine.rs:586,662) pass empty.
  4. `CreateRun`/`create_run` (http.rs:98) gains `inputs: Option<HashMap<String,String>>`; resolve
     each against the graph's `VariableDecl` type via `parse_variable_value`.
  5. Control `run` (workflows.rs:1194) forwards an optional `{ inputs }` body to flow.
  6. `data.runGraph` (data.ts:353) gains optional `inputs`; `runNow` (+page.svelte:1572) opens a modal
     pre-filled from the params' defaults when any `param` var exists, else runs unchanged.
  - Cron keeps using defaults (correct). Deferred: the queued-manual-run drain (engine.rs:170) runs
    with defaults in v1.

## Phase 3 — Typed-column Dictionary table

Owner wants columns-with-types + typed cells + row CRUD, which the string→string `Dict` can't hold.

- **Data model**: introduce a typed table value rather than overload `Dict` (HttpPost still needs
  string→string headers/fields, domain.rs:574). Recommended: a new `PortType::Table` +
  `PortValue::Table { columns: Vec<(String, PortType)>, rows: Vec<Vec<PortValue>> }` (start with
  Text/Num/Bool cell types), serialized to JSON for the variable `default` string. (Alternative:
  extend `Dict` — rejected, it breaks HttpPost.) This is the phase's biggest/riskiest piece — the
  cell-type set and JSON schema are the sub-decision to nail first.
- **Editor**: a new `table-editor.svelte` (sibling to `dictionary-editor.svelte`) — add/remove/rename
  columns with a type each, add/remove/edit rows with per-column typed cell inputs. Wired as the
  default editor for a `Table` variable and (later) as the node-config editor for Table ports.
- Scope note: a Table-consuming node isn't required for v1 — the variable + editor + round-trip is
  the deliverable; wiring Table into node ports is a follow-up.

## Phase 4 — Template catalog

- **Storage**: the six curated templates as graphs in a reserved system tenant (or a static seed
  loaded at flow start), each with its cameras/recipients as `param` variables read by GetVariable
  nodes (retiring the camera-literal problem: the port is wired from a param the *new user* sets).
- **Endpoints** (control, reuse `mutate` + duplicate logic): `GET /workflows/templates` (list: id,
  name, description, the param decls) and `POST /workflows/templates/:id/use` (clone into the caller's
  tenant, new uuid, `enabled=false`, variables incl. `param`/`default`/`description` verbatim).
- **UI**: a template picker (list → "Use" → lands in the Setup form to fill inputs → enable). The
  landing already advertises "start from a template" (#100) — this makes it real.
- **Authoring**: for each of the 6 real workflows, replace camera/recipient config literals with a
  `param` VariableDecl + a GetVariable wired into the port.

## Open sub-decisions (implementation-level, decide at build)

1. **Table cell types + JSON schema** (Phase 3) — start Text/Num/Bool; lock the serialized shape.
2. **File port run-time read** (Phase 1b) — fetch-from-MinIO-by-key now, or defer to the #94 value
   store. Lean: store the key now, wire the read as a small follow-up.
3. **Queued manual run inputs** (Phase 2) — v1 drains with defaults; stash inputs in the queue entry
   later if it matters.

## Verification

- **Latency**: add/rename/retype/delete a variable — the row changes **instantly**, no spinner, and
  a background PUT persists it; no full-list refetch (watch the network tab: one `PUT /variables`,
  no `GET /workflows`). Reload the page → the change stuck.
- **Typed editors**: a Recipient var's default is a dropdown defaulting to the first recipient; a File
  var uploads and shows the filename, with the object in MinIO; a Table var opens the typed table and
  round-trips its columns/rows through save+reload.
- **Layout**: name field no longer dominates; type is readable; gaps look right at narrow rail widths.
- **Input variables**: mark a var as a param → it appears in Setup; run the graph manually → prompted
  with the default pre-filled → an override is used for that run only; cron/event runs use the
  default.
- **Templates**: `GET /workflows/templates` lists the six; "Use" clones one in, disabled, with its
  camera/recipient params empty; fill them in Setup, enable, and it runs on the user's cameras with
  no graph editing.
- Existing workflow tests stay green (`cargo test -p app-core --lib workflows`); add a test that
  `parse_variable_value` matches default-seeding, and that a manual-run override beats the default.

## Out of scope (v1)

- Table-consuming nodes; typed cells beyond Text/Num/Bool.
- Required/validation, enum constraints, input ordering/grouping.
- Cross-tenant template sharing / marketplace; template versioning / "update from template".
- Per-invocation inputs for cron/event (they use defaults, by design).
