// Hit-testing in world/grid space for the map editor's Select tool. All picking is
// done here in JS (not via three raycasting) so selection, drag and the draw tools
// share one consistent coordinate model (the Viewport's screen→world).
import type { MapMarker, MapRoom, MapShape, MapText, MapWall } from "$lib/types";
import type { Pt } from "./tools.svelte";

export type Hit = { kind: "room" | "wall" | "marker" | "text" | "shape"; id: number } | null;
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

function pointInTriangle(px: number, pz: number, ax: number, az: number, bx: number, bz: number, cx: number, cz: number): boolean {
  const d1 = (px - bx) * (az - bz) - (ax - bx) * (pz - bz);
  const d2 = (px - cx) * (bz - cz) - (bx - cx) * (pz - cz);
  const d3 = (px - ax) * (cz - az) - (cx - ax) * (pz - az);
  const hasNeg = d1 < 0 || d2 < 0 || d3 < 0;
  const hasPos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(hasNeg && hasPos);
}

/** Nearest wall endpoint within grab range — for reshaping a wall by an end. */
export function nearWallEndpoint(p: Pt, walls: MapWall[]): EndpointHit {
  for (const w of walls) {
    if (Math.hypot(p.x - w.x1, p.z - w.y1) <= ENDPOINT_GRAB) return { id: w.id, which: 1 };
    if (Math.hypot(p.x - w.x2, p.z - w.y2) <= ENDPOINT_GRAB) return { id: w.id, which: 2 };
  }
  return null;
}

/** Topmost entity under the point: texts, shapes, markers, walls, then rooms. */
export function hitTest(
  p: Pt,
  rooms: MapRoom[],
  walls: MapWall[],
  markers: MapMarker[],
  texts: MapText[],
  shapes: MapShape[],
): Hit {
  for (let i = texts.length - 1; i >= 0; i--) {
    const t = texts[i];
    if (Math.hypot(p.x - t.x, p.z - t.y) <= 0.7) {
      return { kind: "text", id: t.id };
    }
  }

  for (let i = shapes.length - 1; i >= 0; i--) {
    const s = shapes[i];
    if (s.kind === "circle" && s.r != null) {
      if (Math.hypot(p.x - s.x, p.z - s.y) <= s.r) return { kind: "shape", id: s.id };
    } else if (s.kind === "rect" && s.w != null && s.h != null) {
      const rad = ((s.rot ?? 0) * Math.PI) / 180;
      const dx = p.x - s.x, dz = p.z - s.y;
      const c = Math.cos(rad), sn = Math.sin(rad);
      const lx = dx * c + dz * sn;
      const lz = -dx * sn + dz * c;
      if (Math.abs(lx) <= s.w / 2 && Math.abs(lz) <= s.h / 2) return { kind: "shape", id: s.id };
    } else if (s.kind === "triangle" && s.pts && s.pts.length === 3) {
      const [a, b, c] = s.pts;
      if (pointInTriangle(p.x, p.z, a[0], a[1], b[0], b[1], c[0], c[1])) {
        return { kind: "shape", id: s.id };
      }
      // also hit near any edge
      if (
        distToSegment(p, a[0], a[1], b[0], b[1]) < 0.3 ||
        distToSegment(p, b[0], b[1], c[0], c[1]) < 0.3 ||
        distToSegment(p, c[0], c[1], a[0], a[1]) < 0.3
      ) {
        return { kind: "shape", id: s.id };
      }
    }
  }

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
    const cx = r.x + r.w / 2;
    const cz = r.y + r.h / 2;
    const rad = ((r.rot ?? 0) * Math.PI) / 180;
    const dx = p.x - cx;
    const dz = p.z - cz;
    const c = Math.cos(rad);
    const s = Math.sin(rad);
    const lx = dx * c - dz * s;
    const lz = dx * s + dz * c;
    if (Math.abs(lx) <= r.w / 2 && Math.abs(lz) <= r.h / 2) {
      return { kind: "room", id: r.id };
    }
  }
  return null;
}
