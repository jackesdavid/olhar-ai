# GetCameraRoom — Implementation Plan

**Board card:** GetCameraRoom node (ToDo)
**Goal:** Look up which floor-plan room a camera is assigned to and output the room name; signal not-found via an `ok: Bool` output.

## Context

- `RoomCamera` references cameras by `ip` (the stable ONVIF device IP).
- `CameraEntity` (output of `GetCameraByName` / motion events) carries `id`, `ip`, `name`.
- Rooms are loaded per-tenant via `map::repo::load(db, tenant_id)` → `Vec<MapRoom>`.
- The established pattern for fallible lookups (`GetLastAlert`, `GetRecipient`) is: data outputs + `ok: Bool`. No exec branching on the node itself — consumers wire `ok → If`.

## Phase 1 — Backend (domain + execution)

**`crates/app-core/src/workflows/domain.rs`**

1. Add `GetCameraRoom` to `NodeKind::ALL` (after `BreakCamera`).
2. Match the doc comment + input/output port spec in the arms of `node_inputs`, `node_outputs`, `node_display_name`, and `node_description`:
   - Input: `camera: Camera` (required)
   - Outputs: `room_name: Text`, `ok: Bool`
   - Display name: `"Get Camera Room"`
   - Category: can share with `GetCameraByName` / `BreakCamera` (camera utilities)

**`crates/app-core/src/workflows/node.rs`**

3. Add `run_get_camera_room` function:
   ```rust
   fn run_get_camera_room(inputs, sys, config) -> NodeOutputs {
       let cam = inputs.get_camera("camera")?;        // CameraEntity
       let rooms = map::repo::load(&sys.db, sys.tenant_id);
       let found = rooms.iter().find(|r| {
           r.cameras.iter().any(|rc| rc.ip == cam.ip)
       });
       match found {
           Some(room) => outputs!(room_name: room.name.clone(), ok: true),
           None       => outputs!(room_name: String::new(), ok: false),
       }
   }
   ```
4. Wire it into the `run_node` match arm for `NodeKind::GetCameraRoom`.

## Phase 2 — Frontend

**`sentinel-app/src/routes/workflows/+page.svelte`** (or the node-registry file)

5. Register `get_camera_room` in the node palette — label `"Get Camera Room"`, category `"Camera"`.
6. No property panel needed — the node has no config (input arrives via port).

## Open decisions

- None — the lookup key (`ip`) and output shape (`room_name + ok`) are unambiguous given the existing data model.

## Out of scope (v1)

- Returning which `MapRoom.id` was found (name is sufficient for display / alerting use).
- Multiple rooms per camera (a camera can technically be in multiple rooms; v1 returns the first match).
- A `room_id: Num` output — add later if needed for joins.
