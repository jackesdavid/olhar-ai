// Map palette — architectural monochrome: near-black ground, pure-white walls.
// Matches the "plano de planta" reference: dark drafting ground + white walls.
// Emerald is reserved exclusively for LIVE / interactive accents — selection,
// camera FOV, presence pulses — so the live state pops against the neutral plan.

/** Near-black ground — the dark drafting paper the plan sits on. */
export const FLOOR_BASE = "#13171b";
/** Subtle white box-grid lines over the ground (graph-paper feel). */
export const GRID_INK = "rgba(255, 255, 255, 0.07)";
/** Soft focal glow layered over the grid. */
export const GLOW_INK = "160, 185, 200"; // rgb triplet for radial gradient stops

/** Pure white — wall body fill. */
export const INK_FILL = "#ffffff";
/** Pure white — wall edges and furniture outlines. */
export const INK = "#ffffff";

/** Muted dark slate for room zones + labels — barely above the ground. */
export const SLATE_LINE = "#3a4752";
export const SLATE_FILL = "#1d252d";
export const LABEL = "#7a8c96";

/** Emerald — LIVE / interactive accents only (selection, FOV, pulses). */
export const ACCENT = "#34d399";
export const ACCENT_BRIGHT = "#6ee7b7";
export const ACCENT_SOFT = "#10b981";
export const ACCENT_LABEL = "#d1fae5";
