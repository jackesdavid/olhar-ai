# Tracking + editor fixes found debugging the line-crossing workflow — Implementation Plan

**Board card:** #140 Tracking job resolution + editor/box-settings gaps
**Goal:** Make an `OnEvent(object_entered/object_exited/line_crossed)` node reliably produce a
tracking job — today it can silently produce nothing, with zero feedback, and a routine edit deletes
a working job — plus the editor/box-settings gaps the same debugging session exposed.

**Provenance:** every item below was reproduced live on tenant `default` (box `MacInstalledBox`,
camera `Sala`) while getting the seeded "Cruzamento de linha (perímetro)" template to fire. The
chain itself is proven working end-to-end: an `object_entered` test produced two `ok` runs
(`02:47:52`, `02:47:53`, `ran 4 node(s)`), so edge tracker → control → event queue → `OnEvent` → run
is sound. What follows are the defects around it.

## Evidence (live debugging session, tenant `default`)
An operator cloned the "Cruzamento de linha (perímetro)" template, wired the Region (2 points,
named), set `class: person`, enabled the graph — and **tracking never started**. `tracking_config
.trackers` stayed `[]` for ~1h of debugging with no error anywhere.

Three distinct defects combined:

### 1. The camera never persisted, and could not be persisted
`sync_tracking_config` (`sentinel-control/src/domain/workflows.rs`) drops a node whose camera does
not resolve: `let Some(camera) = g.resolve_camera_selector(n) else { continue };`.
`resolve_camera_selector` (`crates/app-core/src/workflows/domain.rs`) reads a wired
`GetCameraByName`'s config, else the node's own `camera` config — **empty strings are filtered out**.
The stored config was literally `{"camera": ""}` while **the panel displayed "Sala"**, because
`cameraValue()` resolves an empty stored value to the *first camera* for display only. With a
**single-camera tenant the Select can never fire `onValueChange`** (picking the value it already
shows isn't a change) → the config was **impossible to set through the UI**.
*Partly fixed already* — the editor now auto-persists the displayed camera (commit `b4e68ae`), but
that effect is gated on `editLocked`, so it never runs on an **armed** graph (exactly the graphs
that need tracking).

### 2. Failure is silent
A dropped job produces no warning in the editor, no box health signal, nothing. The only clue is a
box-side log line (`object tracking disabled: no tracking jobs configured`) the operator never sees.

### 3. Disabling a graph deletes its tracking job
`sync_tracking_config` only scans `graphs.iter().filter(|g| g.enabled)`. Because `editLocked` blocks
editing an armed graph, the normal edit loop is **disable → edit → save → re-enable** — and the save
in the middle **wipes `trackers[]`**. Forgetting the last step leaves the box silently untracked;
this repeatedly invalidated live tests during the session.

## Phase 1 — Resolve the camera the way the UI already promises
- `resolve_camera_selector`: treat an empty/missing camera as **`"first"`**, mirroring the existing
  display fallback AND the precedent right above it in `sync_tracking_config`, which already does
  exactly this for `class` (`unwrap_or("person")`, with a comment explaining the same hazard).
- Retroactive: fixes every already-saved graph with no user action and no disable/enable dance.
- **Trade-off to accept deliberately:** an enabled graph with an untouched tracking `OnEvent` will
  now start a job on the first camera (CPU on the box) where it previously did nothing. This is what
  the panel already claims is selected, so it matches the user's mental model.

## Phase 2 — Make the failure visible
- Editor: flag an `OnEvent(object_entered/object_exited/line_crossed)` that will not yield a job
  ("not scoped to a camera — tracking won't start"), in the node/inspector, like `requiredUnfilled`.
- Optionally surface the box's "no tracking jobs configured" state in box health.

## Phase 3 — Don't lose the job on a routine edit
Decide one of:
- keep extracting from disabled graphs but mark those jobs inactive, or
- let the editor edit an armed graph's node config without disabling, or
- warn on disable ("this stops tracking on <box>") and on save-while-disabled.

## Phase 4 — Seeded templates ship without a camera
`sentinel-control/src/seed_templates.rs`: "Cruzamento de linha", "Clipe em movimento" and "Presença
viva no Mapa" seed with no camera, so every clone starts in this trap. Seed `camera: "first"` and
sharpen the in-canvas doc ("1) escolha a câmera 2) desenhe a linha (2 pontos) 3) salve e habilite").

## Phase 5 — Remaining touch-broken `<datalist>` pickers
The When Event picker was already fixed (commit `867e6b6`): `<input list>` + `<datalist>` **never
opens on iPad/iOS Safari** and is inconsistent on Android, so on a tablet it read as an empty text
box with no hint any event existed — and the only way through was typing the exact name, where a
typo silently created a phantom event that never fires. Two more of the same remain in
`sentinel-app/src/routes/workflows/[[id]]/+page.svelte`:
- `wf-groups` — the workflow **Group** field
- `wf-event-cats` — the event **category** field in the registration form

Both are lower severity (free text, optional value) but share the symptom. Replace with the same
`Select` pattern, keeping free text where a new value must be creatable.

## Phase 6 — Box tracking settings: `detect_ms` has no UI, and the API is destructive
- **`detect_ms` is unreachable from the app.** It is box-global (`tenant_boxes.tracking_config
  .detect_ms`, default 400) and has **zero references anywhere in `sentinel-app/src`** — the only
  way to change it is `PATCH /boxes/:id/tracking-config`. It is the highest-impact tracking knob
  (detection cadence ⇒ how far a subject moves between cycles ⇒ whether a track survives a
  crossing), so it should be editable alongside the per-node tuning.
- **The PATCH is a full replace, not a merge.** `store::set_tracking_config` does
  `UPDATE tenant_boxes SET tracking_config = $1`, and the handler passes the request body straight
  through — so `{"detect_ms": 200}` **wipes `trackers[]`** and stops tracking until the next graph
  save. (The reverse direction is already safe: the graph-save sync reads the box's current
  `enabled`/`detect_ms` and overwrites only `trackers`.) Make it a merge, or validate/preserve the
  keys it doesn't own.

## Out of scope
- Routine tracker calibration (`detect_ms`/`iou_thresh`/`min_hits` values for a given camera).
- The line-crossing geometry itself — `check_crossings` (`crates/app-core/src/tracking.rs`) is
  correct: it needs a **Confirmed** track with a `prev_centre` whose `prev→cur` segment properly
  intersects the line segment in that cycle. Making a real crossing register reliably is a
  continuity/tuning matter (the session observed the track identity swapping mid-walk:
  `b411d650… lost` → `2e68629e… entered`), not a code defect.
