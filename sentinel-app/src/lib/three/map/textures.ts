// Canvas-generated textures for the map floor. Built on the client (they touch
// <canvas>), so call these from onMount / browser code only. Colours are passed in
// (from the theme-reactive mapInk) so the floor tracks light/dark.
import { CanvasTexture, RepeatWrapping, type Texture } from "three";

/** Two-level DOT lattice — a dot at every minor cell, a larger/brighter dot at the
 * major intersection. Echoes the workflow editor's dot-field grid (the dot is the
 * atom of the Sentinel identity). One major tile = 5 world units. */
export function dotGridTexture(minorInk: string, majorInk: string): Texture {
  const MINOR = 64; // px per minor cell (one world unit)
  const MAJOR_N = 5; // minor cells per major division
  const S = MINOR * MAJOR_N; // 320px tile

  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  ctx.clearRect(0, 0, S, S);

  // Minor dots at every cell corner within the tile.
  ctx.fillStyle = minorInk;
  for (let x = 0; x < MAJOR_N; x++) {
    for (let y = 0; y < MAJOR_N; y++) {
      ctx.beginPath();
      ctx.arc(x * MINOR, y * MINOR, 1.4, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  // Major dot at the tile origin — bigger + brighter (the 5-unit division).
  ctx.fillStyle = majorInk;
  ctx.beginPath();
  ctx.arc(0, 0, 2.6, 0, Math.PI * 2);
  ctx.fill();

  const tex = new CanvasTexture(c);
  tex.wrapS = tex.wrapT = RepeatWrapping;
  return tex;
}

/** Soft radial glow — brighter/denser at the focal centre, fading out. Layered
 * over the dot grid to give depth and draw the eye to the plan centre. */
export function radialGlowTexture(glowInk: string): Texture {
  const S = 512;
  const c = document.createElement("canvas");
  c.width = c.height = S;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
  g.addColorStop(0, `rgba(${glowInk}, 0.18)`);
  g.addColorStop(0.5, `rgba(${glowInk}, 0.05)`);
  g.addColorStop(1, `rgba(${glowInk}, 0)`);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, S, S);
  return new CanvasTexture(c);
}
