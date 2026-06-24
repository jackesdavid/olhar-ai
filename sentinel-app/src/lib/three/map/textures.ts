// Canvas-generated textures for the map floor. Built on the client (they touch
// <canvas>), so call these from onMount / browser code only.
import { CanvasTexture, RepeatWrapping, type Texture } from "three";
import { GRID_INK, GLOW_INK } from "$lib/map/palette";

/** Two-level grid tile — minor lines every cell, major lines every 5 cells.
 * One major tile = 5 world units. Grid lines are darker than the workflow
 * editor to stay understated behind 3D geometry. */
export function boxGridTexture(): Texture {
  const MINOR = 64;   // px per minor cell (one world unit)
  const MAJOR_N = 5;  // minor cells per major division
  const S = MINOR * MAJOR_N; // 320px tile

  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  ctx.clearRect(0, 0, S, S);

  // Minor grid lines — 4 internal lines per axis at very low opacity
  ctx.strokeStyle = "rgba(255,255,255,0.04)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = 1; i < MAJOR_N; i++) {
    const v = i * MINOR - 0.5;
    ctx.moveTo(v, 0); ctx.lineTo(v, S); // vertical
    ctx.moveTo(0, v); ctx.lineTo(S, v); // horizontal
  }
  ctx.stroke();

  // Major grid lines — right + bottom edge of the tile, slightly brighter
  ctx.strokeStyle = "rgba(255,255,255,0.09)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(S - 0.5, 0); ctx.lineTo(S - 0.5, S);
  ctx.moveTo(0, S - 0.5); ctx.lineTo(S, S - 0.5);
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
