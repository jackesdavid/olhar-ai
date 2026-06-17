// Curated furniture / landmark icon library for map markers. Each icon is a small
// SVG (filled silhouettes, 24×24) parsed by three's SVGLoader into flat geometry
// and rendered as an emerald glyph on the floor — vector-crisp at any zoom and
// trivially extensible: add an entry here and it shows up in the picker.

// `anchor` is the icon's logical origin in viewBox (0–24) coords — the point that lands
// on the placement target and that the marker rotates around. Defaults to the center
// [12, 12]; override it for icons whose meaningful pivot is off-center (e.g. a door's
// hinge), so they align onto walls correctly. Future swapped-in assets just declare it.
export type IconDef = { key: string; label: string; svg: string; anchor?: [number, number] };

const VB = (body: string) => `<svg viewBox="0 0 24 24" fill="currentColor">${body}</svg>`;

const CENTER: [number, number] = [12, 12];

export const MARKER_ICONS: IconDef[] = [
  {
    key: "sofa",
    label: "Sofa",
    svg: VB(
      '<rect x="3" y="10" width="18" height="7" rx="2"/><rect x="2.5" y="7" width="4" height="8" rx="2"/><rect x="17.5" y="7" width="4" height="8" rx="2"/>',
    ),
  },
  {
    key: "bed",
    label: "Bed",
    svg: VB('<rect x="3" y="11" width="18" height="6" rx="1"/><rect x="5" y="7.5" width="7" height="4.5" rx="1"/>'),
  },
  {
    key: "table",
    label: "Table",
    svg: VB('<rect x="4" y="9" width="16" height="3" rx="1"/><rect x="5" y="12" width="2" height="6"/><rect x="17" y="12" width="2" height="6"/>'),
  },
  {
    key: "tv",
    label: "TV",
    svg: VB('<rect x="3" y="5" width="18" height="11" rx="1"/><rect x="9" y="18" width="6" height="1.6" rx="0.5"/>'),
  },
  {
    key: "plant",
    label: "Plant",
    svg: VB('<circle cx="12" cy="9" r="6"/><rect x="11" y="14" width="2" height="6"/>'),
  },
  {
    key: "car",
    label: "Car",
    svg: VB('<rect x="3" y="11" width="18" height="5" rx="2"/><path d="M6 11 l2 -3.5 h8 l2 3.5 z"/><circle cx="7.5" cy="17" r="2"/><circle cx="16.5" cy="17" r="2"/>'),
  },
  {
    key: "sink",
    label: "Sink / bath",
    svg: VB('<rect x="3" y="10" width="18" height="7" rx="3.5"/><circle cx="12" cy="7.5" r="1.4"/>'),
  },
  {
    key: "stairs",
    label: "Stairs",
    svg: VB('<rect x="4" y="16" width="16" height="3"/><rect x="8" y="12" width="12" height="3"/><rect x="12" y="8" width="8" height="3"/><rect x="16" y="4" width="4" height="3"/>'),
  },
  {
    key: "fridge",
    label: "Fridge",
    svg: VB('<rect x="7" y="3" width="10" height="18" rx="1.5"/><rect x="14" y="6" width="1.6" height="3" rx="0.5" fill="#fff"/>'),
  },
  {
    key: "door",
    label: "Door swing",
    // Architectural door symbol: a leaf (the open door) + a quarter-circle swing arc.
    // Anchor = the hinge (top of the leaf), so the door pivots + aligns there onto a wall.
    svg: VB(
      '<rect x="4.4" y="4" width="1.7" height="15.3"/><path d="M5.3 4 A15 15 0 0 1 20.3 19.3 L18.5 19.3 A13.2 13.2 0 0 0 5.3 6.1 Z"/>',
    ),
    anchor: [5.25, 4.5],
  },
  {
    key: "arc",
    label: "Swing arc",
    // A generic semicircle band — a movement / opening indicator. Anchor = the centre of
    // the flat base (the edge that sits against a wall).
    svg: VB('<path d="M3 18 A9 9 0 0 1 21 18 L19 18 A7 7 0 0 0 5 18 Z"/>'),
    anchor: [12, 18],
  },
];

const BY_KEY = new Map(MARKER_ICONS.map((i) => [i.key, i]));

export function iconSvg(key: string): string {
  return (BY_KEY.get(key) ?? MARKER_ICONS[0]).svg;
}

export function iconLabel(key: string): string {
  return (BY_KEY.get(key) ?? MARKER_ICONS[0]).label;
}

/** The icon's logical origin in viewBox coords (defaults to the center). */
export function iconAnchor(key: string): [number, number] {
  return (BY_KEY.get(key) ?? MARKER_ICONS[0]).anchor ?? CENTER;
}
