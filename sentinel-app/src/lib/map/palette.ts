// Map palette — the "architectural neutral" floor-plan look. The geometry (floor,
// walls, rooms, furniture) reads as crisp near-white line-art on a cool blue-slate
// ground, like a printed plano de planta. Emerald is reserved for LIVE / interactive
// accents ONLY — selection, camera FOV, presence pulses — so the live state pops
// against the neutral plan. Keep three-consumable hex here (three reads hex directly).

/** Cool blue-slate ground — the paper the plan is drawn on. */
export const FLOOR_BASE = "#2b353d";
/** Faint cool dot-field / grid ink over the ground (was emerald). */
export const GRID_INK = "rgba(186, 201, 212, 0.35)";
/** Soft cool focal glow layered over the dot field. */
export const GLOW_INK = "148, 170, 186"; // rgb triplet for the radial gradient stops

/** Near-white "drawn ink" — wall edges, furniture outlines. */
export const INK = "#e8eef0";
/** Filled wall body (solid walls), a touch below pure ink. */
export const INK_FILL = "#cdd6dc";

/** Muted slate for secondary geometry — room zones + labels. */
export const SLATE_LINE = "#5b6770";
export const SLATE_FILL = "#3a464e";
export const LABEL = "#8a97a0";

/** Emerald — LIVE / interactive accents only (selection highlight, FOV, pulses). */
export const ACCENT = "#34d399";
export const ACCENT_BRIGHT = "#6ee7b7";
export const ACCENT_SOFT = "#10b981";
export const ACCENT_LABEL = "#d1fae5";
