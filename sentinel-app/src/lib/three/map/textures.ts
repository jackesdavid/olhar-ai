// Canvas-generated textures for the map floor. Built on the client (they touch
// <canvas>), so call these from onMount / browser code only.
import { CanvasTexture, RepeatWrapping, type Texture } from "three";
import { GRID_INK, GLOW_INK } from "$lib/map/palette";

/** Box-grid tile — a single square cell with faint white border lines, repeated
 * across the floor. One tile = one world unit. Matches the drafting-paper grid
 * look of the architectural floor-plan reference. */
export function boxGridTexture(): Texture {
  const S = 64; // px per grid cell in the tile
  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  ctx.clearRect(0, 0, S, S);
  ctx.strokeStyle = GRID_INK;
  ctx.lineWidth = 1;
  // Draw the right and bottom edges only — tiling fills the rest.
  ctx.beginPath();
  ctx.moveTo(S - 0.5, 0);
  ctx.lineTo(S - 0.5, S);
  ctx.moveTo(0, S - 0.5);
  ctx.lineTo(S, S - 0.5);
  ctx.stroke();
  const tex = new CanvasTexture(c);
  tex.wrapS = tex.wrapT = RepeatWrapping;
  return tex;
}

/** Soft radial glow — brighter/denser at the focal centre, fading out. Layered
 * over the box grid to give depth and draw the eye to the plan centre. */
export function radialGlowTexture(): Texture {
  const S = 512;
  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
  g.addColorStop(0, `rgba(${GLOW_INK}, 0.18)`);
  g.addColorStop(0.5, `rgba(${GLOW_INK}, 0.05)`);
  g.addColorStop(1, `rgba(${GLOW_INK}, 0)`);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, S, S);
  return new CanvasTexture(c);
}
