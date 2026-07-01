// Map palette — architectural monochrome. The theme-reactive ink (floor, walls,
// grid, zones, labels) now lives in ./palette.svelte.ts (`mapInk`), so the map
// tracks the app's light/dark theme like the workflow editor. This file keeps only
// the FIXED emerald accents — reserved exclusively for LIVE / interactive state
// (selection, camera FOV, presence pulses) so live pops against the neutral plan.

/** Emerald — LIVE / interactive accents only (selection, FOV, pulses). */
export const ACCENT = "#34d399";
export const ACCENT_BRIGHT = "#6ee7b7";
export const ACCENT_SOFT = "#10b981";
export const ACCENT_LABEL = "#d1fae5";
