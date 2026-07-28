// Pure presentation helpers for the workflows list/editor (labels, glyphs, relative time).
// No Svelte, no reactive state. Extracted from the editor component (#137).
//
// The text-producing helpers take their wording as a `labels` argument rather than reaching for
// the i18n dict, so they stay pure and unit-testable — callers pass `dict[lang()].workflowsList`.

/** The subset of the `workflowsList` messages these helpers need. */
export type FormatLabels = {
  justNow: string;
  minutesAgo: (n: number) => string;
  hoursAgo: (n: number) => string;
  daysAgo: (n: number) => string;
  running: string;
  triggerCron: string;
  triggerEvent: string;
  triggerManual: string;
};

/** Relative "time ago" for an RFC3339 timestamp. */
export function timeAgo(iso: string, l: FormatLabels): string {
  const then = new Date(iso).getTime();
  if (!Number.isFinite(then)) return "";
  const s = Math.max(0, (Date.now() - then) / 1000);
  if (s < 60) return l.justNow;
  const m = Math.floor(s / 60);
  if (m < 60) return l.minutesAgo(m);
  const h = Math.floor(m / 60);
  if (h < 24) return l.hoursAgo(h);
  return l.daysAgo(Math.floor(h / 24));
}

/** Glyph for a run status (last-run column). Language-independent. */
export function lastRunGlyph(status: string): string {
  return status === "ok" ? "✓" : status === "failed" ? "✗" : "•";
}

/** Tailwind text-colour class for a run status. */
export function lastRunClass(status: string): string {
  if (status === "failed") return "text-destructive";
  if (status === "running") return "text-primary";
  return "text-muted-foreground";
}

/** Human label for a run's trigger source (`cron`/`event`/`manual`). */
export function triggerSourceLabel(src: string | null | undefined, l: FormatLabels): string {
  if (src === "cron") return l.triggerCron;
  if (src === "event") return l.triggerEvent;
  if (src === "manual") return l.triggerManual;
  return l.running;
}
