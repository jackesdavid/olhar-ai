// Theme-reactive architectural ink for the 3D map — so the map editor tracks the
// same light/dark theme as the workflow editor and the two feel like one product.
//
//   • light "Clean White"  → dark ink on light paper (a classic floor plan)
//   • dark  "Sentinel Noir" → white ink on dark paper (a blueprint negative)
//
// three.js needs concrete colour values, not CSS custom properties, so this file
// MIRRORS the elevation ramp in styles/sentinel-tokens.css rather than reading it.
// Every surface below names the stop it mirrors — keep them in step, and keep them
// NEUTRAL: chroma 0, like the rest of the suite. (This file used to carry a blue
// cast — #3a4752 / #1d252d / #7a8c96 — which broke the "no hue on surfaces" rule.)
//
// The map floor is the same recessed canvas as the workflow editor's: elev-0.
//
// The emerald ACCENT* hues (live presence, camera FOV, selection) stay fixed and
// live in ./palette.ts — the one reserved hue, exactly like the workflow port colours.
import { theme } from "$lib/theme.svelte";

type Ink = {
  floor: string; // ground plane (the "paper")
  gridMinor: string; // minor dot-lattice ink (rgba)
  gridMajor: string; // major dot-lattice ink (rgba)
  glow: string; // focal radial glow (rgb triplet for gradient stops)
  wallFill: string; // wall body
  wallEdge: string; // wall + furniture outlines
  slateLine: string; // room-zone stroke
  slateFill: string; // room-zone fill
  label: string; // floor labels
};

const LIGHT: Ink = {
  floor: "#e8e8e8", // elev-0 — light drafting paper
  gridMinor: "rgba(20,20,20,0.10)",
  gridMajor: "rgba(20,20,20,0.17)",
  glow: "20, 20, 20", // subtle dark focal shading
  wallFill: "#1f1f1f", // dark ink walls on light paper
  wallEdge: "#0f0f0f",
  slateLine: "#bcbcbc",
  slateFill: "#e2e2e2",
  label: "#6b6b6b", // ≈ --muted-foreground
};

const DARK: Ink = {
  floor: "#0a0a0a", // elev-0 — the recessed canvas, same as the workflow editor
  gridMinor: "rgba(255,255,255,0.07)",
  gridMajor: "rgba(255,255,255,0.13)",
  glow: "230, 230, 230", // neutral focal glow (was blue-tinted)
  wallFill: "#ffffff", // pure-white walls on dark paper
  wallEdge: "#ffffff",
  slateLine: "#4a4a4a", // room-zone stroke, reads on the elev-0 floor
  slateFill: "#1a1a1a", // ≈ elev-2 — a zone lifts subtly off the floor
  label: "#8a8a8a", // ≈ --muted-foreground
};

function pick<K extends keyof Ink>(k: K): string {
  return (theme.current === "dark" ? DARK : LIGHT)[k];
}

/** Reactive ink — read a field inside a $derived/template and it tracks the theme. */
export const mapInk = {
  get floor() { return pick("floor"); },
  get gridMinor() { return pick("gridMinor"); },
  get gridMajor() { return pick("gridMajor"); },
  get glow() { return pick("glow"); },
  get wallFill() { return pick("wallFill"); },
  get wallEdge() { return pick("wallEdge"); },
  get slateLine() { return pick("slateLine"); },
  get slateFill() { return pick("slateFill"); },
  get label() { return pick("label"); },
};
