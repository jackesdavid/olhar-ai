# Camera typed port + GetCameraByName node — Implementation Plan

**Board card:** Camera typed port + GetCameraByName node (ToDo)
**Goal:** Introduce a first-class `Camera` port type so camera-carrying edges are type-safe; add a `GetCameraByName` pure-data node that lets a user pick a camera and wire its output to any camera-input node.

---

## Context

Today every node that takes a camera (`CameraSnapshot`, `GetCurrentFrame`, `MotionWatch`) and the motion system event's `camera` payload field all use `PortType::Text`. This means a Text value from an unrelated node can be wired in without a type error, and there is no way to distinguish "a camera reference" from an arbitrary string on the canvas.

The camera value on the wire is the **stable device id** (ONVIF `urn:uuid` or IP fallback) — the same string that config dropdowns already store. Adding a `Camera` PortType changes nothing about the runtime value; it only adds graph-level type checking and a distinct colour on the canvas.

---

## Phase 1 — Add `Camera` PortType (backend)

File: `crates/app-core/src/workflows/domain.rs`

1. Add `Camera` variant to the `PortType` enum (after `Face`, before `Any`):
   ```rust
   /// A camera device reference (stable device id). Carried as a string on the wire.
   Camera,
   ```
2. The enum is `serde(rename_all = "snake_case")` so it serialises as `"camera"` automatically.
3. No change to `compatible()` — `Camera` only matches `Camera` (or `Any`).

---

## Phase 2 — Update camera input ports on existing nodes

File: `crates/app-core/src/workflows/domain.rs`

Change every `p("camera", PortType::Text, ...)` to `p("camera", PortType::Camera, ...)`:

| Const | Line (approx) |
|---|---|
| `CAMERA_SNAPSHOT_IN` | 277 |
| `GET_CURRENT_FRAME_IN` | 284 |
| `MOTION_WATCH_IN` | 373 |

Also check `RecordPresence` (line 264 area) — if it has a camera Text port, update it too.

Runtime is unaffected: the value on the wire is still the device-id string; `run_node` arms already call `resolve_device` which accepts that string.

---

## Phase 3 — Update motion system event payload

File: `crates/app-core/src/workflows/repo.rs`

Line 273: change `field("camera", PortType::Text)` → `field("camera", PortType::Camera)`.

This means `OnEvent("motion")` gains a `camera: Camera` output port. Any downstream node that accepts `Camera` input (e.g. `CameraSnapshot`) can now be wired directly: motion fires → camera out → snapshot in.

---

## Phase 4 — Add `GetCameraByName` node

A **pure data node** (no exec pins): user picks a camera from the config dropdown, and the node outputs it as a `Camera` value. Lets a user inject a static camera reference into any part of a graph without exec flow.

### `domain.rs` changes

1. Add `GetCameraByName` to the `NodeKind` enum.
2. Add to `NodeKind::ALL` (in the Cameras block near `CameraSnapshot`).
3. Port declarations:
   ```rust
   const GET_CAMERA_BY_NAME_IN: [Port; 0] = [];
   const GET_CAMERA_BY_NAME_OUT: [Port; 1] = [p("camera", PortType::Camera, false)];
   ```
4. Slug: `get_camera_by_name`, label: `"Get camera"`, category: `"Cameras"`.
5. Default config: `json!({ "camera": "" })`.
6. No exec pins — do not add it to any exec-pin list (same pattern as `Compare`, `NumLt`, etc.).

### `node.rs` — `run_node` arm

```rust
NodeKind::GetCameraByName => {
    let cam = config_str(cfg, "camera");
    let mut out = NodeOutputs::new();
    out.set("camera", Value::String(cam.to_string()));
    Ok(out)
}
```

The value is the device id from config. Downstream nodes call `resolve_device` as usual.

---

## Phase 5 — Frontend: Camera type colour + TypeScript

File: `sentinel-app/src/routes/workflows/+page.svelte`

1. Add `"camera"` to the `PortType` TypeScript union (line ~53).
2. Add colour entry to `portColor` map (line ~717):
   ```ts
   camera: "#38bdf8",  // sky-400 — distinct from text (blue-400 #60a5fa)
   ```
3. The camera config UI (dropdown to pick a device) on `CameraSnapshot`, `GetCurrentFrame`, `MotionWatch` nodes is unchanged — it's driven by `config.camera`, not by the port type. The dropdown will also appear on `GetCameraByName` automatically (same `camera` config key).

---

## Open decisions

1. **Backward compatibility**: existing graphs with Text → camera-input edges will now fail type-checking. Options: (a) silently accept any existing saved edges on load (relax compatible() for Text↔Camera), (b) let the editor surface the type mismatch and require the user to re-wire. Option (b) is cleaner; the change is intentional.
2. **RecordPresence camera port**: verify whether it has a camera input that also needs updating. If yes, include in Phase 2.

## Out of scope (v1)

- Carrying the full `Device` struct on the wire (just the device id string is enough).
- A `Camera` → `Text` coercion node (users can use `Print`/`Format` if they need the string).
- Camera array type (`CameraArray`) for multi-camera fan-out.
