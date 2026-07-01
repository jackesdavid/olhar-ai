// Theme-reactive architectural ink for the 3D map — so the map editor tracks the
// same light/dark theme as the workflow editor and the two feel like one product.
//
//   • light "Clean White"  → dark ink on light paper (a classic floor plan)
//   • dark  "Sentinel Noir" → white ink on dark paper (a blueprint negative)
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
  floor: "#e9e9ec", // ~ --well light — light drafting paper
  gridMinor: "rgba(20,20,22,0.10)",
  gridMajor: "rgba(20,20,22,0.17)",
  glow: "20, 20, 24", // subtle dark focal shading
  wallFill: "#1c1f24", // dark ink walls on light paper
  wallEdge: "#0e1013",
  slateLine: "#b4bcc4",
  slateFill: "#dde2e7",
  label: "#5c6870",
};

const DARK: Ink = {
  floor: "#13171b", // near-black drafting ground
  gridMinor: "rgba(255,255,255,0.06)",
  gridMajor: "rgba(255,255,255,0.11)",
  glow: "160, 185, 200",
  wallFill: "#ffffff", // pure-white walls on dark paper
  wallEdge: "#ffffff",
  slateLine: "#3a4752",
  slateFill: "#1d252d",
  label: "#7a8c96",
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
