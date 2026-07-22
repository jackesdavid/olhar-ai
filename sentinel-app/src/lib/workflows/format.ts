// Pure presentation helpers for the workflows list/editor (labels, glyphs, relative time).
// No Svelte, no reactive state. Extracted from the editor component (#137).

/** Relative "time ago" for an RFC3339 timestamp. */
export function timeAgo(iso: string): string {
  const then = new Date(iso).getTime();
  if (!Number.isFinite(then)) return "";
  const s = Math.max(0, (Date.now() - then) / 1000);
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

/** Glyph for a run status (last-run column). */
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
export function triggerSourceLabel(src?: string | null): string {
  if (src === "cron") return "Cron";
  if (src === "event") return "Event";
  if (src === "manual") return "Manual";
  return "Running";
}
