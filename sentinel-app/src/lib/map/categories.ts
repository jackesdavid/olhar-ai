// Single source of truth for presence categories → colour + label. Used by both the
// map's pulse (the "ping") and the legend. Colours are on-palette (emerald is home;
// the rest map to the chart tokens) as hex so three can consume them directly.
import type { PresenceCategory } from "$lib/types";

export type CategoryStyle = { color: string; label: string };

export const CATEGORY_STYLE: Record<PresenceCategory, CategoryStyle> = {
  known_person: { color: "#34d399", label: "Known person" }, // emerald --primary
  person: { color: "#fbbf24", label: "Person" }, // amber
  car: { color: "#38bdf8", label: "Car" }, // sky
  cat: { color: "#a78bfa", label: "Cat" }, // violet
  dog: { color: "#fb923c", label: "Dog" }, // orange
  unknown: { color: "#94a3b8", label: "Unknown" }, // muted
};

export const CATEGORY_ORDER: PresenceCategory[] = [
  "known_person",
  "person",
  "car",
  "cat",
  "dog",
  "unknown",
];

export function categoryColor(c: PresenceCategory): string {
  return CATEGORY_STYLE[c]?.color ?? CATEGORY_STYLE.unknown.color;
}

export function categoryLabel(c: PresenceCategory): string {
  return CATEGORY_STYLE[c]?.label ?? "Unknown";
}
