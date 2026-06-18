// Canvas-generated textures for the map floor — the "dot-field" identity, in cool
// blueprint tones (see lib/map/palette.ts). Built on the client (they touch <canvas>),
// so call these from onMount / browser code only.
import { CanvasTexture, RepeatWrapping, type Texture } from "three";
import { GRID_INK, GLOW_INK } from "$lib/map/palette";

/** Cool dot tile, repeated across the floor — a blueprint grid. Returns a texture
 * set to repeat; the caller sets `.repeat` to the world size. */
export function dotFieldTexture(): Texture {
  const S = 64; // px per grid cell in the tile
  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  ctx.clearRect(0, 0, S, S);
  // One soft cool dot per cell, centred.
  ctx.fillStyle = GRID_INK;
  ctx.beginPath();
  ctx.arc(S / 2, S / 2, 2.4, 0, Math.PI * 2);
  ctx.fill();
  const tex = new CanvasTexture(c);
  tex.wrapS = tex.wrapT = RepeatWrapping;
  return tex;
}

/** Soft cool radial glow — denser/brighter at the focal centre, fading out. Layered
 * over the dot field to give the dot-field its radial falloff. */
export function radialGlowTexture(): Texture {
  const S = 512;
  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
  g.addColorStop(0, `rgba(${GLOW_INK}, 0.22)`);
  g.addColorStop(0.5, `rgba(${GLOW_INK}, 0.06)`);
  g.addColorStop(1, `rgba(${GLOW_INK}, 0)`);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, S, S);
  return new CanvasTexture(c);
}
