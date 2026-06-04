# Spec: Floor-Plan Map (Rooms & Camera Placement)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/map/`.
> Documents existing behavior; no code change implied.

## Goal

The operator lays out a floor plan of rooms and places cameras within them on a grid canvas.

---

## User Value

A spatial mental model of the site — which camera covers which room — so the operator reasons
about coverage and incidents by location, not by camera name.

---

## Requirements

- Grid canvas editor with modes: Browse, Move, Resize, Rename, Cameras (assign), CamPlace.
- Create/select/move/resize/rename axis-aligned room rectangles.
- Assign cameras to a room and position each at a (dx, dy) offset from the room corner.
- Pan and zoom across discrete zoom levels; keep the selected room in view.
- Persist rooms to `map.json` and viewport (pan/zoom) to `mapview.json`.

---

## Acceptance Criteria

- [x] Operator can add a room and move/resize/rename it.
- [x] Operator can assign cameras to a room and place them at offsets.
- [x] Pan and zoom work across the discrete zoom levels.
- [x] Rooms persist to `map.json`; viewport persists to `mapview.json`.
- [x] The canvas renders rooms and camera markers (y-flipped Braille grid).

---

## Dependencies

- 002-camera-discovery (camera list to assign)

---

## Risks

- Grid/coordinate math (y-down origin, zoom) is error-prone — covered by unit tests.

---

## Notes

`MapRoom { id, name, x, y, w, h, cameras[] }`, `RoomCamera { ip, dx, dy }`. Grid 64×36, 5 zoom
levels (16/24/32/48/64 wide, default 32). This canvas-editor pattern is the model the
Workflows editor (spec 013) mirrors.
