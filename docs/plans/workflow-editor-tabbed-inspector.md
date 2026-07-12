# Workflow editor — tabbed right rail — Implementation Plan

**Board card:** #75 Workflow editor: tabbed right rail (Details / Properties / Variables / Runs) (ToDo)
**Design standard:** the `/impeccable` skill. House pattern to mirror: `routes/map/components/SidePanel.svelte`.
**Goal:** Collapse the editor's side chrome into **one tabbed right rail** — workflow settings, the selected
node's properties, graph variables, and run history become peers you switch between instead of things you
scroll past or hunt for in a modal. **The left rail goes away entirely**, giving the canvas back ~224px.

**Tabs (decided):** **Details** (workflow config) · **Properties** (selected node) · **Variables** · **Runs**.
The Console stays a full-width bottom panel.

---

## Where we are today

The editor (`sentinel-app/src/routes/workflows/[[id]]/+page.svelte`, ~4250 lines) is a flex row (**L2671**):
`VariablesPanel` (left) │ canvas (`flex-1`) │ splitter │ **right column**.

The right column (**L3329–4018**) is a single `overflow-y-auto` scroller stacking **two always-visible panels**:

| Panel | Lines | Shows |
|---|---|---|
| **Configuration** | 3334–3465 | Workflow-level: name, description, group, enabled, orphans, debug, trigger (+cron), concurrency (+max), delete-workflow |
| **Node property / inspector** | 3467–4017 | The selected node's config; `{:else}` an empty "Select a node…" state |

So on any non-trivial node you scroll past the entire workflow config to reach the properties you actually want.
That's the problem.

**What already exists (and must be preserved):**
- Rail width `panelW` (**L349**, 240–560px, persisted `localStorage["wf.panelW"]`), resize handlers **L362–381**.
- Rail collapse `panelOpen` (**L350**), `togglePanel()` (**L352–359**), persisted `localStorage["wf.panelOpen"]`;
  collapsed = a 7px reopen strip (**L3299–3309**).

**Run history is currently a modal Dialog** (`runsOpen`, **L4164–4224**) reachable **only from the workflow *list*
row button** (**L2541**) — not from inside the editor at all. It is the obvious third tab and the cheapest win here.

**The inspector is schema-driven, not a `NodeKind` switch** — good news for portability. `configRows`
(`$derived.by`, **L702–718**) derives rows from the catalog's `default_config` + input ports; the markup
`{#each configRows}` (**L3484**) picks a widget from a long `{:else if}` chain keyed on `key`/`port.type`
(camera, recipient, dictionary, file, image, bool, level, room, category, class, points→`RegionPointsEditor`, …),
plus four kind-specific sub-editors: event payload (3838–3883), string/AI switch arms (3887–3935), camera switch
arms (3936–3993), memoize (3997–4011).

**There is no shadcn `Tabs` primitive** in `lib/components/ui/` — but we don't need one. `routes/map/components/
SidePanel.svelte` is already the house pattern: a collapsible right `<aside>` with **icon buttons as tabs**
(`tab = $state<Tab>(…)`, `aria-pressed`), a `PanelRightClose` collapse button, and localStorage persistence of
**both** `map.panelTab` and `map.panelOpen`. Mirror it; don't add a dependency.

---

## Phase 1 — Extract the two panels into components

No behaviour change. Pure move, so the tab work lands on a clean surface.

- New `routes/workflows/components/ConfigPanel.svelte` ← markup from **L3334–3465**.
- New `routes/workflows/components/PropertiesPanel.svelte` ← markup from **L3467–4017**, including the four
  kind-specific sub-editors.
- **The inspector closes over ~25 page-level things** — `catalog`, `graph`, `editLocked`, `configRows`,
  `cameraOptions`/`recipientOptions`/`roomOptions`/`categoryOptions`, `rooms`, `events`, `eventCategories`,
  `newEventCategory`, `newEventFields`, `COCO_CLASSES`, `LEVELS`, `TRACKING_TUNING_DEFAULTS`/`_HELP`, `NO_ROOM`,
  and mutators `saveNodeConfig`, `liveSaveConfig`, `saveNodeMemoize`, `removeNode`, `unlinkPort`, `pickFile`,
  `pickImage`, `wiredInto`, `resolvedCameraIp`, `payloadFields`, `setEventFieldName/Type`. A 25-prop `$props()`
  signature is not acceptable — see **Open decision 3**.
- Mirror the file layout of `routes/map/components/` (SidePanel + PropertiesPanel + ExplorerPanel).

## Phase 2 — The tabbed rail

- New `routes/workflows/components/InspectorRail.svelte`, modelled on `map/components/SidePanel.svelte`:
  an icon-button tab strip at the top of the rail, `aria-pressed`, then the active tab's body.
- Replaces **L3329–4018** wholesale. The HUDs are all `absolute` inside the *canvas* column (**L3211–3296**), so
  none of them are touched — but **budget vertical space at the top of the rail**: the zoom/pan HUD sits at
  `absolute right-3 top-3` (**L3275–3284**), roughly 12px from the rail's left edge, and a tab strip flush with
  the rail's top will sit shoulder-to-shoulder with it. Either inset the tab strip or nudge the zoom HUD.
- Keep `panelW` resize + `panelOpen` collapse exactly as they are. Add `wf.panelTab` to localStorage alongside
  `wf.panelOpen` / `wf.panelW`, matching the map's `map.panelTab` precedent.
- The rail header keeps the existing `PanelRightClose` button (~**L3343**).

## Phase 3 — Mount / unmount policy (this one has teeth)

Not all tabs can be unmounted when inactive:

- **Properties → hide with CSS, do not unmount.** Two reasons. `RegionPointsEditor`
  (`components/RegionPointsEditor.svelte:24–28,49`) holds `imgSrc`/`objectUrl` and **fetches a camera snapshot in
  an `$effect`, revoking the object URL on teardown** — unmounting re-fetches the frame on every tab return
  (visible flicker + a network round-trip per switch). And the rail's `overflow-y-auto` **scroll position is
  DOM-owned and lost on unmount**.
- **Config / Runs / Variables → unmount freely.** Nothing stateful. (Variables' only local state is the width
  `$state` and its localStorage `$effect` — both of which Phase 4 deletes anyway, since it inherits the rail's
  width. The variables themselves live on `graph`, at page level.)
- Everything else already survives, because it's page-level, not panel-level: the `logs` buffer (**L244**), the
  `onEvent("workflow-run")` **SSE subscription is registered in the page's `onMount`** (**L1250**), `liveNodes`
  (**L457**), `screenMsgs` (**L248**), `activity` (**L428**), the `timeGateTimer` interval (**L317–321**).
  Inspector edits are **not buffered** — every widget calls `saveNodeConfig`/`liveSaveConfig` on change, so there
  is no in-progress edit to lose on a switch.
- One thing to verify: `newEventCategory` (**L434**) / `newEventFields` (**L436**) are page-level and survive, but
  they're **seeded from the selected node by an `$effect`** — confirm that effect doesn't clobber a half-typed
  event payload when you leave and return to the tab.

## Phase 4 — Variables moves from the left rail into a tab

The left rail disappears. `VariablesPanel` becomes the **Variables** tab body, and the canvas grows by its width.

- **The drag survives the move.** Variable rows use **native HTML5 drag & drop** — `draggable` +
  `ondragstart` writing a custom `application/x-sentinel-variable` MIME payload
  (`components/VariablesPanel.svelte:100–103`), which the canvas drop handler reads to drop a Get/Set node.
  Native DnD doesn't care which side of the canvas the drag starts from, so right-to-left works unchanged.
  This was the main risk in moving it; it's a non-issue.
- **Strip the panel's own chrome — it now inherits the rail's.** Delete: the `open = $bindable(true)` prop
  (**VariablesPanel.svelte:20, 29**), its 7px reopen rail (**60–66**), its "Hide variables" button (**80**), its
  own `width` `$state(224)` + resize handle (**37–52, 166**) and the `wf-vars-w` localStorage key. The tab body
  fills the rail and is sized by the rail's existing `panelW`.
- Remove `varsOpen` from `+page.svelte` (**L2673–2683**) and delete the left column from the flex row (**L2671**).
- The left-edge HUDs (`Add node` at **L3211**, tool palette at **L3218**) are `absolute` inside the canvas
  column, so they simply sit closer to the viewport edge. No change needed, but eyeball it.
- Keep the empty state ("No variables yet. Add one, then drag it onto the canvas…", **L88**) — it's the only
  thing that teaches the drag gesture.

## Phase 5 — Runs becomes a tab

- Move the run-history body out of the modal Dialog (**L4164–4224**) into the **Runs** tab; reuse `openRuns()`
  (**L1880**) and `toggleRunLogs()` (**L1892**).
- Keep the Dialog for the **list** view (**L2541**) — that entry point has no rail to put a tab in. So the body
  becomes a shared component used by both the tab and the dialog.
- This is the first time run history is reachable *without leaving the editor*, which is most of the value of
  this card.

## Phase 6 — Selection behaviour + polish

- **Selecting a node switches to the Properties tab** (this is what Unreal's Blueprint editor does, and it's the
  whole reason the tab split works). See **Open decision 2** for the exact rule.
- Empty state on Properties when nothing is selected: keep the existing "Select a node to edit its properties"
  (**L4013–4016**) but render it via `EmptyState` (the dot-field mark + one line), per `/impeccable`.
- Tab strip gets a subtle count/indicator where it helps — e.g. a dot on **Runs** when a run is live
  (`liveNodes`), so you know to look without switching.
- `npm run build` + `task check`. Walk the rail at 240px and 560px (`PANEL_MIN`/`PANEL_MAX`, **L347–348**).

---

## Open decisions (grill before building)

1. ~~**Which tabs, exactly?**~~ **Decided:** **Details · Properties · Variables · Runs**. Variables moves out of
   the left rail into the tab strip, so the editor ends up with a single side rail and a wider canvas. This
   diverges from Unreal (which keeps My Blueprint on the left), and that's a deliberate call — one rail beats two.
   - **Console stays a full-width bottom panel** (`consoleOpen` **L243**, **L4043–4085**). It wants the width, and
     it's the one piece of Unreal's layout worth keeping. Still open if you want it folded in later.
2. **Does clicking a node force the Properties tab?** Now sharper, because with four tabs you're more likely to be
   sitting somewhere else when you click a node. Options: (a) always switch — simplest, matches UE, but it
   yanks you out of Details mid-edit; (b) switch only if the current tab is Properties-or-empty; (c) never switch,
   just badge the tab. I lean **(a)**, because selecting a node *is* the intent to inspect it — but it's a taste
   call and it's the one users will feel most.
3. **How does `PropertiesPanel` get its ~25 dependencies?** (a) a wide `$props()` — honest but ugly; (b) a Svelte
   **context** object set once by the page (`setContext("wf-editor", …)`) — clean call sites, weaker types;
   (c) bundle them into one `editor` prop object. I lean **(c)** — one typed prop, explicit, no context magic,
   and it keeps `PropertiesPanel` testable in isolation.
4. **What happens to card #61** ("Tab toggle: collapse/expand editor side panels", plan at
   `[[editor-panel-tab-toggle]]`, still ToDo)? Its plan assumes **three independent panels** (vars + right +
   console) and proposes a Tab-key group collapse. This card changes the thing it's written against. Either fold
   #61 into this card, or re-plan it after this lands — **it should not be built first**.

## Out of scope (v1)

- The Console stays a full-width bottom panel.
- No change to the schema-driven `configRows` widget chain — it moves, it doesn't get rewritten.
- No change to the variables *feature* (typed vars, Get/Set drop) — `VariablesPanel` relocates and sheds its own
  width/collapse chrome; its body and the drag payload are untouched.
- No new shadcn `Tabs` dependency — we mirror `map/components/SidePanel.svelte`.
- **Cleanup worth noting, not doing here:** `routes/workflows/components/NodePalette.svelte` is **dead code** —
  not imported anywhere in `src/`; `openAddNodeMenu` (**L2370–2380**) opens `ContextSearch` instead. Delete it in
  a separate sweep.
