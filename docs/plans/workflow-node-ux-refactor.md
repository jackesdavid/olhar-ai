# Workflow Node UX Refactor — Rename, Reorganize & Split into Files

**Board card:** #55 Workflow nodes — rename, categorize & split into files (ToDo)
**Goal:** Make every node name instantly discoverable and self-explanatory to a non-developer user; clean up categories; split the 1,839-line `node.rs` into per-category modules.

---

## Current state (51 nodes, 12 categories)

All node execution lives in one 1,839-line `node.rs`; all kind metadata in one 1,670-line `domain.rs`.  
The frontend palette derives directly from the backend category + label, so renaming the backend renames the UI.

---

## Phase 1 — Node renames (label only, no behavior change)

Changes to `label()` in `domain.rs`. Enum variant names stay the same to keep existing graph JSON valid.

### Cameras (4 nodes)

| Variant | Current label | Proposed label | Reason |
|---|---|---|---|
| `GetCameraByName` | "Get camera" | **"Find Camera"** | "Get" is vague; "Find" implies a lookup |
| `CameraSnapshot` | "Camera snapshot" | **"Snapshot"** | Category is already "Cameras"; redundant prefix |
| `GetCurrentFrame` | "Get current frame" | **"Current Frame"** | Flag as potential duplicate of Snapshot (see Open decisions) |
| `BreakCamera` | "Break camera" | **"Camera Details"** | "Break" is Rust/Blueprint jargon; "Details" is instantly understood |

### Vision / Detection (10 nodes)

| Variant | Current label | Proposed label | Reason |
|---|---|---|---|
| `PersonDetection` | "Person detection" | **"Detect People"** | Active verb, consistent with ObjectDetection pattern |
| `ObjectDetection` | "Detect objects" | **"Detect Objects"** | Capitalize consistently |
| `MotionDetection` | "Motion detection" | **"Detect Motion"** | Active verb, consistent |
| `MotionWatch` | "Motion watch" | **"Watch for Motion"** | Distinguishes it from Detect Motion — this is a reactive trigger, not a one-shot |
| `RecognizePerson` | "Recognize person" | **"Recognize Person"** | Already clear — keep |
| `BreakPersonDetection` | "Break person detection" | **"Unpack Person"** | "Break" is jargon; "Unpack" is universal |
| `BreakDetection` | "Break detection" | **"Unpack Detection"** | Same |
| `BreakFaceDetection` | "Break face detection" | **"Unpack Face"** | Same |
| `DrawBbox` | "Draw BBox" | **"Draw Detection Box"** | BBox is ML jargon |
| `GetImageSize` | "Get image size" | **"Image Size"** | Shorter; pure data node, no "Get" verb needed |

### AI (2 nodes)

| Variant | Current label | Proposed | Reason |
|---|---|---|---|
| `DescribeImage` | "Describe image" | **"Describe Image"** | Keep — already clear |
| `AskAi` | "AI If" | **"AI Branch"** | "AI If" is ambiguous; "AI Branch" shows it routes execution |

### Alerts (3 nodes)

| Variant | Current label | Proposed | Reason |
|---|---|---|---|
| `GetLastAlert` | "Get last alert" | **"Last Alert"** | Pure data node — noun form, no "Get" verb |
| `CreateAlert` | "Create alert" | **"Create Alert"** | Keep |
| `DeliverAlert` | "Deliver alert" | **"Send Alert"** | "Deliver" is internal/technical; "Send" is user language |

### Recipients (2 nodes)

| Variant | Current label | Proposed | Reason |
|---|---|---|---|
| `GetRecipient` | "Get recipient" | **"Find Recipient"** | "Find" implies lookup by name/ID |
| `NotifyRecipient` | "Notify recipient" | **"Notify"** | Category is already "Recipients" — no redundant noun |

### Control (was "Flow") — 9 nodes + category rename

Category "Flow" → **"Control"** — "Control" is the standard term (Unreal, Node-RED, etc.)

| Variant | Current label | Proposed | Reason |
|---|---|---|---|
| `If` | "If" | **"If"** | Universal — keep |
| `ForEach` | "For each" | **"For Each"** | Keep |
| `StringSwitch` | "String switch" | **"Text Switch"** | "String" is dev jargon; "Text" is user language |
| `FlipFlop` | "Flip flop" | **"Toggle"** | "Toggle" is universally understood |
| `Cooldown` | "Cooldown" | **"Cooldown"** | Keep |
| `Wait` | "Wait" | **"Wait"** | Keep |
| `TimeGate` | "Time Gate" | **"Time Gate"** | Keep — already clear |
| `EmitEvent` | "Emit event" | **"Send Event"** | "Emit" is dev jargon |
| `OnEvent` | "On event" | **"When Event"** | "When" is natural language; consistent with "When motion…" mental model |

### Logic (13 nodes)

| Variant | Current label | Proposed | Reason |
|---|---|---|---|
| `Compare` | "Compare" | **"Compare"** | Keep |
| `TextCompare` | "Text compare" | **"Text Compare"** | Keep (see Open decisions — may overlap with Compare) |
| `NumLt` | "Num: <" | **"Less Than"** | Plain English |
| `NumLte` | "Num: <=" | **"Less Than or Equal"** | Plain English |
| `NumGt` | "Num: >" | **"Greater Than"** | Plain English |
| `NumGte` | "Num: >=" | **"Greater Than or Equal"** | Plain English |
| `NumEq` | "Num: ==" | **"Number Equal"** | Disambiguates from text equality |
| `NumNe` | "Num: !=" | **"Number Not Equal"** | Plain English |
| `NumToText` | "Num → Text" | **"Number to Text"** | Spelled out |
| `BoolToText` | "Bool → Text" | **"Yes/No to Text"** | "Bool" is dev jargon |
| `And` | "And" | **"And"** | Keep |
| `Or` | "Or" | **"Or"** | Keep |
| `Not` | "Not" | **"Not"** | Keep |

### New category: Debug (split from Flow + Utility)

Move `Print`, `ShowImage`, `Comment` out of Flow/Utility into a new **"Debug"** category.  
These are canvas inspection tools — grouping them together makes them easy to find and also signals "these don't run in production logic."

### Other categories (no renames)

- **Text** (Format) — keep as is
- **Network** (HttpPost) — keep as is; consider "HTTP Request" if PUT/PATCH support added later
- **Presence** (RecordPresence) — keep label; rename category if Presence gets more nodes
- **Recording** (StartRecording, StopRecording) — keep

---

## Phase 2 — Category restructure in `domain.rs`

1. Rename enum variant `Flow` → `Control` in the `category()` return values
2. Add `Debug` variant; re-assign `Print`, `ShowImage`, `Comment`
3. Update `CATEGORY_ORDER` in `NodePalette.svelte` to include "Debug" and "Control"
4. Update `docs/workflow-nodes.md` headings + node names table

---

## Phase 3 — Split `node.rs` into per-category modules

### Target layout: `crates/app-core/src/workflows/nodes/`

```
nodes/
  mod.rs          — pub fn run_node() dispatch match only
  camera.rs       — CameraSnapshot, GetCurrentFrame, GetCameraByName, BreakCamera
  vision.rs       — PersonDetection, ObjectDetection, MotionDetection, MotionWatch,
                    RecognizePerson, BreakPersonDetection, BreakDetection,
                    BreakFaceDetection, DrawBbox, GetImageSize
  ai.rs           — DescribeImage, AskAi
  alerts.rs       — GetLastAlert, CreateAlert, DeliverAlert
  recipients.rs   — GetRecipient, NotifyRecipient
  control.rs      — If, ForEach, FlipFlop, Toggle (StringSwitch), Cooldown,
                    Wait, TimeGate, EmitEvent, OnEvent
  logic.rs        — Compare, TextCompare, NumLt/Lte/Gt/Gte/Eq/Ne,
                    NumToText, BoolToText, And, Or, Not
  network.rs      — HttpPost
  text.rs         — Format
  recording.rs    — StartRecording, StopRecording
  presence.rs     — RecordPresence
  debug.rs        — Print, ShowImage, Comment
```

`node.rs` becomes `nodes/mod.rs` with only the `run_node()` dispatch and `node_timeout()`.  
Each module file gets the handler functions that belong to it.  
No public API changes — `run_node()` signature stays identical.

### Optionally split `domain.rs` too

`domain.rs` (1,670 lines) can follow a similar split later if desired:
- `domain/kind.rs` — `NodeKind` enum, `slug()`, `label()`, `category()`, `is_pure()`
- `domain/ports.rs` — `PortType`, `Port`, `exec_in()`/`exec_out()`/`data_in()`/`data_out()`
- `domain/graph.rs` — `WorkflowGraph`, `Node`, `Edge`, execution model types

This is optional/follow-on — Phase 3 (node.rs split) is the higher-value change.

---

## Open decisions

1. **`CameraSnapshot` vs `GetCurrentFrame`** — do these do the same thing? If yes, deprecate one; if no, the new names should make the distinction obvious (e.g. "Snapshot" saves to disk; "Live Frame" reads from the live feed).

2. **`Compare` vs `TextCompare`** — `Compare` appears to be generic; `TextCompare` appears text-specific. Are they duplicates? If `Compare` already handles text, `TextCompare` is redundant and can be removed with a serde alias for backward compat.

3. **6 Num* nodes → one "Number Compare" node** — long-term UX win: one node with an operator dropdown (`<`, `<=`, `>`, `>=`, `==`, `!=`). Deferred to a follow-up card (requires behavior change, new config schema, migration). Flag for v2.

4. **`RecordPresence` rename** — "Track Presence" or "Log Presence" would be clearer, but the node does a specific map-presence write. Confirm with product before renaming.

---

## Out of scope (v1)

- Merging the 6 Num* nodes into one (behavior change, own card)
- Adding new nodes or ports
- Changing any port types or execution model
- `domain.rs` split (optional follow-on)
- Updating existing saved graph JSON (labels are display-only; slugs in JSON stay the same)
