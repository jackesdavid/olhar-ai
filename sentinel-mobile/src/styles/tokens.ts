// The design tokens, mirrored from the shared web/site source of truth: `styles/sentinel-tokens.css`
// at the repo root (consumed by `sentinel-app` and `landing`). The phone is another window onto the
// same product, so it must read identically — never invent a colour here.
//
// The web file is authored in `oklch`, which React Native cannot parse, so values are converted to
// sRGB hex. The conversion is exact (oklch → oklab → linear sRGB → gamma-encoded), and the dark ramp
// round-trips to the hex values written in that file's own comments (#0A0A0A, #0F0F0F, #151515 …),
// which is the cheap way to confirm nothing drifted.
//
// **When `styles/sentinel-tokens.css` changes, re-derive these** — they are a copy, not a link.
// There is no build step tying them together; that is the trade for RN not understanding CSS vars.

/** Elevation ramp — pick a stop, never a hand-rolled surface value. Same roles as the web. */
export type Elevation = {
  /** recessed — code, log, scrollbar track */ elev0: string;
  /** sunken — inputs (recessed below the card) */ elev1: string;
  /** base — page canvas */ elev2: string;
  /** panel — cards (lifts off the page) */ elev3: string;
  /** raised — chips, nested panel */ elev4: string;
  /** float — popover */ elev5: string;
  /** hover */ elev6: string;
  /** active / selected */ elev7: string;
};

export type Theme = Elevation & {
  well: string;
  background: string;
  card: string;
  muted: string;
  inputBg: string;
  foreground: string;
  mutedForeground: string;
  primary: string;
  primaryForeground: string;
  destructive: string;
  border: string;
  input: string;
  ring: string;
};

const lightRamp: Elevation = {
  elev0: "#E8E8E8",
  elev1: "#F0F0F0",
  elev2: "#F3F3F3",
  elev3: "#FFFFFF",
  elev4: "#F7F7F7",
  elev5: "#FFFFFF",
  elev6: "#EEEEEE",
  elev7: "#E8E8E8",
};

const darkRamp: Elevation = {
  elev0: "#0A0A0A",
  elev1: "#0F0F0F",
  elev2: "#151515",
  elev3: "#1E1E1E",
  elev4: "#242424",
  elev5: "#2F2F2F",
  elev6: "#3E3E3E",
  elev7: "#575757",
};

export const lightTheme: Theme = {
  ...lightRamp,
  well: lightRamp.elev0,
  background: lightRamp.elev2,
  card: lightRamp.elev3,
  muted: lightRamp.elev4,
  inputBg: lightRamp.elev1,
  foreground: "#161616",
  mutedForeground: "#555555",
  primary: "#0B0B0B",
  primaryForeground: "#FCFCFC",
  destructive: "#DF202E",
  // The web uses translucent ink for edges so borders sit on any surface. RN supports 8-digit hex.
  border: "#26262619",
  input: "#26262624",
  ring: "#262626",
};

export const darkTheme: Theme = {
  ...darkRamp,
  well: darkRamp.elev0,
  background: darkRamp.elev2,
  card: darkRamp.elev3,
  muted: darkRamp.elev4,
  inputBg: darkRamp.elev1,
  foreground: "#E8E8E8",
  mutedForeground: "#8C8C8C",
  primary: "#FFFFFF",
  primaryForeground: "#0A0A0A",
  destructive: "#FF6367",
  border: "#FFFFFF24",
  input: "#FFFFFF2E",
  ring: "#FFFFFF",
};

/** `--radius: 0.375rem` on the web — tight/sharp, a deliberate "serious product" feel. */
export const radius = { sm: 2, md: 4, lg: 6, xl: 10, pill: 999 } as const;

/** Spacing scale, kept small and predictable so screens compose without magic numbers. */
export const space = { xs: 4, sm: 8, md: 12, lg: 16, xl: 24, xxl: 32 } as const;

/** Type scale. The web wordmark font (Sora) is not bundled yet, so headings use the system face. */
export const type = {
  display: { fontSize: 30, fontWeight: "600", letterSpacing: -0.5 },
  title: { fontSize: 20, fontWeight: "600", letterSpacing: -0.3 },
  body: { fontSize: 15, lineHeight: 21 },
  caption: { fontSize: 12 },
} as const;
