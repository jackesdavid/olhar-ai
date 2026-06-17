// Hit-testing in world/grid space for the map editor's Select tool. All picking is
// done here in JS (not via three raycasting) so selection, drag and the draw tools
// share one consistent coordinate model (the Viewport's screen→world).
import type { MapMarker, MapRoom, MapWall } from "$lib/types";
import type { Pt } from "./tools.svelte";

export type Hit = { kind: "room" | "wall" | "marker"; id: number } | null;
export type EndpointHit = { id: number; which: 1 | 2 } | null;

const ENDPOINT_GRAB = 0.35;

function distToSegment(p: Pt, ax: number, az: number, bx: number, bz: number): number {
  const dx = bx - ax;
  const dz = bz - az;
  const len2 = dx * dx + dz * dz;
  if (len2 < 1e-9) return Math.hypot(p.x - ax, p.z - az);
  let t = ((p.x - ax) * dx + (p.z - az) * dz) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p.x - (ax + t * dx), p.z - (az + t * dz));
}

/** Nearest wall endpoint within grab range — for reshaping a wall by an end. */
export function nearWallEndpoint(p: Pt, walls: MapWall[]): EndpointHit {
  for (const w of walls) {
    if (Math.hypot(p.x - w.x1, p.z - w.y1) <= ENDPOINT_GRAB) return { id: w.id, which: 1 };
    if (Math.hypot(p.x - w.x2, p.z - w.y2) <= ENDPOINT_GRAB) return { id: w.id, which: 2 };
  }
  return null;
}

/** Topmost entity under the point: markers, then walls, then room zones. */
export function hitTest(
  p: Pt,
  rooms: MapRoom[],
  walls: MapWall[],
  markers: MapMarker[],
): Hit {
  for (let i = markers.length - 1; i >= 0; i--) {
    const m = markers[i];
    if (Math.hypot(p.x - m.x, p.z - m.y) <= 0.6 * (m.scale || 1)) {
      return { kind: "marker", id: m.id };
    }
  }
  for (let i = walls.length - 1; i >= 0; i--) {
    const w = walls[i];
    if (distToSegment(p, w.x1, w.y1, w.x2, w.y2) <= Math.max(w.thickness, 0.25)) {
      return { kind: "wall", id: w.id };
    }
  }
  for (let i = rooms.length - 1; i >= 0; i--) {
    const r = rooms[i];
    if (p.x >= r.x && p.x <= r.x + r.w && p.z >= r.y && p.z <= r.y + r.h) {
      return { kind: "room", id: r.id };
    }
  }
  return null;
}
