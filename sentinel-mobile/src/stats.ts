// What the dashboard counts.
//
// One read of the product event stream, sliced locally. `/events` is already the tenant's timeline
// of everything that happened — motion, clips, faces, approvals, sign-ins — so the phone asks once
// for a window and does its own arithmetic rather than asking the server five different questions.
import { api } from "./session";

export type EventRow = {
  id: number;
  kind: string;
  source: string;
  severity: string;
  title: string | null;
  message: string | null;
  camera: string | null;
  clip_id: string | null;
  occurred_at: string;
};

/** A day's worth of counts, oldest first — what the bars draw. */
export type DayBucket = { day: string; label: string; count: number };

export type Stats = {
  /** Events in the window, newest first. The activity list reads from this too. */
  recent: EventRow[];
  today: number;
  week: number;
  /** Clips recorded in the window. */
  clips: number;
  /** Distinct cameras that reported anything — "how much of the site is awake". */
  cameras: number;
  /** Seven buckets, oldest → newest. */
  perDay: DayBucket[];
  /** The busiest single day in the window, so the bars have a scale. */
  peak: number;
};

const DAY_MS = 86_400_000;

function dayKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/**
 * The last seven days.
 *
 * `limit` is deliberately generous but bounded: a busy site can produce thousands of events a day,
 * and a phone charting a week does not need every one of them — it needs the shape. The counts are
 * "in the sample", which is honest as long as nothing here claims to be a total.
 */
export async function loadStats(): Promise<Stats> {
  const since = new Date(Date.now() - 6 * DAY_MS);
  since.setHours(0, 0, 0, 0);
  const rows = await api<EventRow[]>(
    `/events?from=${encodeURIComponent(since.toISOString())}&limit=1000`,
  );

  const buckets = new Map<string, number>();
  for (let i = 6; i >= 0; i--) {
    const d = new Date(Date.now() - i * DAY_MS);
    buckets.set(dayKey(d), 0);
  }

  const todayKey = dayKey(new Date());
  let today = 0;
  let clips = 0;
  const cameras = new Set<string>();

  for (const e of rows) {
    const when = new Date(e.occurred_at);
    const key = dayKey(when);
    if (buckets.has(key)) buckets.set(key, (buckets.get(key) ?? 0) + 1);
    if (key === todayKey) today++;
    if (e.kind.startsWith("clip")) clips++;
    if (e.camera) cameras.add(e.camera);
  }

  const perDay: DayBucket[] = [...buckets.entries()].map(([day, count]) => ({
    day,
    // One letter per weekday: seven bars on a phone have no room for more.
    label: new Date(`${day}T12:00:00`).toLocaleDateString(undefined, { weekday: "narrow" }),
    count,
  }));

  return {
    recent: rows.slice(0, 12),
    today,
    week: rows.length,
    clips,
    cameras: cameras.size,
    perDay,
    peak: Math.max(1, ...perDay.map((b) => b.count)),
  };
}

/** "3m ago" / "2h ago" / "Tue" — a timestamp someone reads at a glance, not a date they parse. */
export function ago(iso: string, now: number = Date.now()): string {
  const secs = Math.max(0, Math.floor((now - new Date(iso).getTime()) / 1000));
  if (secs < 60) return "just now";
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86_400) return `${Math.floor(secs / 3600)}h ago`;
  return new Date(iso).toLocaleDateString(undefined, { weekday: "short" });
}
