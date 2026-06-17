// Camera field-of-view as an occluded line-of-sight polygon (Option B). On the top-down
// plan a camera "sees" like light spreads: blocked by walls. We sample rays across the FOV
// cone, clip each at the nearest wall (or the camera's range), and return the ring of hit
// points — a polygon that hugs the walls instead of passing through them. Pure 2D world XZ.
import type { MapWall } from "$lib/types";

export type Pt2 = { x: number; z: number };

/** Nearest wall-hit distance along a UNIT ray from (ox,oz), capped at `range`. */
function rayDist(ox: number, oz: number, dx: number, dz: number, range: number, walls: MapWall[]): number {
  let best = range;
  for (const w of walls) {
    const ex = w.x2 - w.x1;
    const ez = w.y2 - w.y1;
    const det = ex * dz - dx * ez;
    if (Math.abs(det) < 1e-9) continue; // parallel
    const ax = w.x1 - ox;
    const az = w.y1 - oz;
    const t = (-ax * ez + ex * az) / det; // distance along the (unit) ray
    const u = (dx * az - dz * ax) / det; // position along the wall segment
    if (t > 1e-4 && t < best && u >= 0 && u <= 1) best = t;
  }
  return best;
}

/**
 * Ring points (world XZ) of the FOV visibility polygon, occluded by `walls`.
 * `facingRad` 0 = +z (matches the camera convention); forward = (-sin, cos).
 */
export function fovPolygon(
  ox: number,
  oz: number,
  facingRad: number,
  fovRad: number,
  range: number,
  walls: MapWall[],
  steps = 64,
): Pt2[] {
  const half = fovRad / 2;
  const n = Math.max(8, Math.round(steps));
  const pts: Pt2[] = [];
  for (let i = 0; i <= n; i++) {
    const a = facingRad - half + (fovRad * i) / n;
    const dx = -Math.sin(a);
    const dz = Math.cos(a);
    const dist = rayDist(ox, oz, dx, dz, range, walls);
    pts.push({ x: ox + dx * dist, z: oz + dz * dist });
  }
  return pts;
}
