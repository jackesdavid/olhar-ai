// Curated furniture / landmark icon library for map markers. Each icon is a small
// SVG parsed by three's SVGLoader into flat geometry and rendered as an emerald glyph
// on the floor — vector-crisp at any zoom and trivially extensible: add an entry here
// and it shows up in the picker.
//
// Two icon modes:
//   svg  — inline SVG string, 24×24 viewBox. Fast, no network.
//   src  — URL to a static SVG asset (e.g. "/markers/car.svg"). Fetched once, cached.
//           Set `vb` to the SVG's largest viewBox dimension so MarkerGlyph scales it to
//           the same world footprint as the inline icons. anchor coordinates are in the
//           external file's viewBox space.
//
// `anchor` is the icon's logical origin — the point that lands on the placement target
// and that the marker rotates around. Defaults to center of the viewBox.
export type IconDef = {
  key: string;
  label: string;
  svg?: string;
  src?: string;
  vb?: number;
  anchor?: [number, number];
};

// The fill value is irrelevant (MarkerGlyph overrides it with the emerald material) — use
// a concrete colour, not `currentColor`, which SVGLoader can't parse (warns on every load).
const VB = (body: string) => `<svg viewBox="0 0 24 24" fill="#9ca3af">${body}</svg>`;

const CENTER: [number, number] = [12, 12];
const CENTER_EXT = (vb: [number, number]): [number, number] => [vb[0] / 2, vb[1] / 2];

// All icons are authored as TOP-DOWN architectural plan symbols, composed of closed
// shapes (rects / circles / paths). The renderer draws each shape as a thin emerald
// outline with a faint fill — the wireframe look from the references.
export const MARKER_ICONS: IconDef[] = [
  {
    key: "sofa",
    label: "Sofa",
    // back rail + seat + two arms (seen from above)
    svg: VB(
      '<rect x="3" y="6" width="18" height="3" rx="1"/><rect x="5" y="8.5" width="14" height="8.5" rx="1.5"/><rect x="3" y="8.5" width="2.5" height="8.5" rx="1"/><rect x="18.5" y="8.5" width="2.5" height="8.5" rx="1"/>',
    ),
  },
  {
    key: "bed",
    label: "Bed",
    // mattress + headboard band + two pillows
    svg: VB(
      '<rect x="4" y="3.5" width="16" height="17" rx="1.5"/><rect x="5.5" y="5" width="13" height="4.5" rx="1"/><rect x="5.5" y="10.5" width="6" height="4" rx="0.8"/><rect x="12.5" y="10.5" width="6" height="4" rx="0.8"/>',
    ),
  },
  {
    key: "table",
    label: "Table",
    // dining table + four chairs around it
    svg: VB(
      '<rect x="6.5" y="7.5" width="11" height="9" rx="1"/><rect x="8.5" y="3.5" width="7" height="2.5" rx="0.6"/><rect x="8.5" y="18" width="7" height="2.5" rx="0.6"/><rect x="3.5" y="9" width="2.5" height="6" rx="0.6"/><rect x="18" y="9" width="2.5" height="6" rx="0.6"/>',
    ),
  },
  {
    key: "tv",
    label: "TV",
    // a thin unit against the wall (top-down), with its screen line
    svg: VB('<rect x="4" y="10.5" width="16" height="3" rx="0.6"/><rect x="5" y="11.3" width="14" height="1.4" rx="0.4"/>'),
  },
  {
    key: "plant",
    label: "Plant",
    // canopy + pot rim
    svg: VB('<circle cx="12" cy="12" r="7"/><circle cx="12" cy="12" r="2.6"/>'),
  },
  {
    key: "car",
    label: "Car",
    // External SVG from static/markers/car.svg.
    // vb = height of the 1932×976 viewBox — normalises the car to the same world height
    // as inline icons while keeping its natural wide-body aspect ratio.
    src: "/markers/car.svg",
    vb: 976,
    anchor: CENTER_EXT([1932, 976]),
  },
  {
    key: "sink",
    label: "Sink / bath",
    // counter + basin + tap
    svg: VB('<rect x="3" y="7" width="18" height="10" rx="1.5"/><rect x="6" y="9" width="12" height="6" rx="3"/><rect x="11" y="6" width="2" height="2" rx="0.5"/>'),
  },
  {
    key: "stairs",
    label: "Stairs",
    // run outline + treads
    svg: VB(
      '<rect x="5" y="3" width="14" height="18"/><rect x="6" y="6" width="12" height="0.8"/><rect x="6" y="9" width="12" height="0.8"/><rect x="6" y="12" width="12" height="0.8"/><rect x="6" y="15" width="12" height="0.8"/><rect x="6" y="18" width="12" height="0.8"/>',
    ),
  },
  {
    key: "fridge",
    label: "Fridge",
    // unit + door divider + handle
    svg: VB('<rect x="6" y="3.5" width="12" height="17" rx="1.5"/><rect x="6" y="10.5" width="12" height="1"/><rect x="14.5" y="6" width="1.2" height="3" rx="0.4"/>'),
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

function def(key: string): IconDef {
  return BY_KEY.get(key) ?? MARKER_ICONS[0];
}

export function iconSvg(key: string): string {
  return def(key).svg ?? '<svg viewBox="0 0 24 24"></svg>';
}

export function iconSrc(key: string): string | null {
  return def(key).src ?? null;
}

export function iconLabel(key: string): string {
  return def(key).label;
}

/** Largest viewBox dimension — used by MarkerGlyph to normalise world scale. */
export function iconVb(key: string): number {
  return def(key).vb ?? 24;
}

/** The icon's logical origin in viewBox coords (defaults to the centre). */
export function iconAnchor(key: string): [number, number] {
  return def(key).anchor ?? CENTER;
}
