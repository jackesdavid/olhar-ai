// Editor tool state + snapping for the floor-plan drawing surface. Coordinates are
// world/grid points on the floor plane ({ x, z }); MapScene turns pointer pixels
// into these via the Viewport, then snaps before drawing.
import type { MapWall } from "$lib/types";

export type ToolMode = "select" | "wall" | "marker";
export type Pt = { x: number; z: number };

const GRID = 1; // snap step in grid units
const VERTEX_SNAP = 0.4; // world-units radius to grab an existing endpoint

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

/** Constrain `to` to a horizontal or vertical line from `from` (whichever is closer). */
export function orthoLock(from: Pt, to: Pt): Pt {
  return Math.abs(to.x - from.x) >= Math.abs(to.z - from.z)
    ? { x: to.x, z: from.z }
    : { x: from.x, z: to.z };
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
