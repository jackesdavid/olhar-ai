// Editor tool state + snapping for the floor-plan drawing surface. Coordinates are
// world/grid points on the floor plane ({ x, z }); MapScene turns pointer pixels
// into these via the Viewport, then snaps before drawing.
import type { MapWall } from "$lib/types";

export type ToolMode = "select" | "wall" | "marker";
export type Pt = { x: number; z: number };

const GRID = 1; // snap step in grid units
const VERTEX_SNAP = 0.65; // world-units radius to grab an existing endpoint
const WALL_SNAP = 0.5; // world-units distance to snap a marker onto a wall line

/** Round a point to the grid. */
export function snapToGrid(p: Pt, step = GRID): Pt {
  return { x: Math.round(p.x / step) * step, z: Math.round(p.z / step) * step };
}

/** All wall endpoints — candidates for vertex snapping (so walls join up). */
export function wallVertices(walls: MapWall[]): Pt[] {
  const out: Pt[] = [];
  for (const w of walls) {
    out.push({ x: w.x1, z: w.y1 }, { x: w.x2, z: w.y2 });
  }
  return out;
}

/** Snap to the nearest existing vertex within the threshold, else null. */
export function snapToVertex(p: Pt, verts: Pt[], threshold = VERTEX_SNAP): Pt | null {
  let best: Pt | null = null;
  let bestD = threshold * threshold;
  for (const v of verts) {
    const d = (v.x - p.x) ** 2 + (v.z - p.z) ** 2;
    if (d <= bestD) {
      bestD = d;
      best = v;
    }
  }
  return best;
}

/** Constrain `to` to the nearest 45° direction from `from` (so Shift gives clean
 * horizontal, vertical AND diagonal walls — like the reference's angled walls). */
export function orthoLock(from: Pt, to: Pt): Pt {
  const dx = to.x - from.x;
  const dz = to.z - from.z;
  const len = Math.hypot(dx, dz);
  if (len < 1e-6) return { ...to };
  const step = Math.PI / 4;
  const ang = Math.round(Math.atan2(dz, dx) / step) * step;
  return { x: from.x + Math.cos(ang) * len, z: from.z + Math.sin(ang) * len };
}

/** Full snap pipeline: ortho-lock (if drawing + shift) → vertex → grid. */
export function resolvePoint(
  raw: Pt,
  opts: { from?: Pt | null; ortho?: boolean; verts: Pt[] },
): Pt {
  let p = raw;
  if (opts.ortho && opts.from) p = orthoLock(opts.from, p);
  const v = snapToVertex(p, opts.verts);
  if (v) return v;
  return snapToGrid(p);
}

/** Project `p` onto a segment, clamped to its ends. */
function projectOnSegment(p: Pt, ax: number, az: number, bx: number, bz: number): Pt {
  const dx = bx - ax;
  const dz = bz - az;
  const len2 = dx * dx + dz * dz;
  if (len2 < 1e-9) return { x: ax, z: az };
  let t = ((p.x - ax) * dx + (p.z - az) * dz) / len2;
  t = Math.max(0, Math.min(1, t));
  return { x: ax + t * dx, z: az + t * dz };
}

/** Snap onto the nearest wall line within the threshold (so a door/marker sits flush
 * on a wall), else null. */
export function snapToWall(p: Pt, walls: MapWall[], threshold = WALL_SNAP): Pt | null {
  let best: Pt | null = null;
  let bestD = threshold;
  for (const w of walls) {
    const proj = projectOnSegment(p, w.x1, w.y1, w.x2, w.y2);
    const d = Math.hypot(p.x - proj.x, p.z - proj.z);
    if (d <= bestD) {
      bestD = d;
      best = proj;
    }
  }
  return best;
}

/** Marker placement snap: prefer landing flush on a nearby wall, else vertex/grid. */
export function resolveMarkerPoint(raw: Pt, walls: MapWall[]): Pt {
  return snapToWall(raw, walls) ?? resolvePoint(raw, { from: null, ortho: false, verts: wallVertices(walls) });
}

/** Editor tool + in-progress draw state (Svelte 5 runes). */
export class ToolState {
  mode = $state<ToolMode>("select");
  /** Pending first endpoint while drawing a wall (null = not mid-segment). */
  draft = $state<Pt | null>(null);
  /** Current snapped cursor position, for the rubber-band preview. */
  hover = $state<Pt | null>(null);
  /** Whether the ortho-lock modifier (Shift) is held. */
  ortho = $state(false);
  /** Selected icon for the marker tool. */
  markerIcon = $state("sofa");

  setMode(m: ToolMode): void {
    this.mode = m;
    this.draft = null;
  }

  cancel(): void {
    this.draft = null;
  }
}
