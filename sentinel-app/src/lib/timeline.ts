// Pure axis math + track layout for the playback timeline (card #67). Side-effect-free so the
// markup stays logic-free and this stays trivially correct — the one source of the time↔pixel
// mapping every phase (ruler, clips, markers, playhead) reads from.
//
// Timestamps are the edge-local RFC3339 the recordings/events queries store; `parseTs` honours
// the offset in the string. The ruler renders in the *viewer's* locale — for the single-tenant
// deployment (viewer and edge share a timezone) that lines up; a future tz-robust pass would
// render straight from the RFC3339 offset.

import type { EventRow, Recording, RecordingCamera } from "$lib/api";

/** Epoch milliseconds for an RFC3339 timestamp (NaN-safe: 0 on a bad parse). */
export function parseTs(iso: string): number {
  const t = Date.parse(iso);
  return Number.isFinite(t) ? t : 0;
}

/** A clip laid on the axis. */
export type TLClip = {
  id: number;
  label: string;
  startTs: number; // epoch ms
  endTs: number; // epoch ms
  uploaded: boolean;
};

/** One camera's row. */
export type TLTrack = {
  cameraIp: string;
  cameraName: string;
  clips: TLClip[];
};

/** Local-day [00:00, 23:59:59] bounds as offset-less RFC3339 prefixes — string-comparable against
 *  the edge-local `started_at` the recordings query filters on (same wall-clock day). */
export function dayBounds(dayISO: string): { from: string; to: string } {
  return { from: `${dayISO}T00:00:00`, to: `${dayISO}T23:59:59` };
}

/** Browser-local midnight (epoch ms) for a `YYYY-MM-DD` day — the axis origin. */
export function dayOrigin(dayISO: string): number {
  return new Date(`${dayISO}T00:00:00`).getTime();
}

/** px position of a timestamp on the axis. */
export function xOf(tsMs: number, originMs: number, pxPerSec: number): number {
  return ((tsMs - originMs) / 1000) * pxPerSec;
}

/** Timestamp at an x offset — the inverse of `xOf` (for the future playhead scrub). */
export function tsOf(x: number, originMs: number, pxPerSec: number): number {
  return originMs + (x / pxPerSec) * 1000;
}

/** Width in px of a span in seconds. */
export function widthOf(durationSecs: number, pxPerSec: number): number {
  return Math.max(2, durationSecs * pxPerSec);
}

/** Group recordings into per-camera tracks, ordered by the camera roster, clips sorted by start.
 *  Cameras that recorded but aren't in the roster are appended (keyed by ip). */
export function buildTracks(items: Recording[], cameras: RecordingCamera[]): TLTrack[] {
  const byIp = new Map<string, TLClip[]>();
  for (const r of items) {
    const startTs = parseTs(r.started_at);
    const clip: TLClip = {
      id: r.id,
      label: r.friendly_name,
      startTs,
      endTs: startTs + r.duration_secs * 1000,
      uploaded: r.upload_state === "uploaded",
    };
    const list = byIp.get(r.camera_ip);
    if (list) list.push(clip);
    else byIp.set(r.camera_ip, [clip]);
  }
  const order: RecordingCamera[] = [...cameras];
  for (const ip of byIp.keys()) {
    if (!order.some((c) => c.ip === ip)) order.push({ ip, name: ip });
  }
  return order
    .filter((c) => byIp.has(c.ip))
    .map((c) => ({
      cameraIp: c.ip,
      cameraName: c.name,
      clips: (byIp.get(c.ip) ?? []).sort((a, b) => a.startTs - b.startTs),
    }));
}

/** A merged run of back-to-back clips — one recording "session" bar on the track. Playback still
 *  resolves the individual clip under the playhead; this is only how the track *renders*, so
 *  motion clips (~40s each, contiguous) read as one continuous block instead of a pile of
 *  identical sub-pixel slivers that overlap under a min-width. */
export type TLSegment = {
  startTs: number;
  endTs: number;
  clipCount: number;
  uploaded: boolean; // every clip in the run is in the cloud
};

/** Merge clips whose neighbour starts within `gapToleranceSecs` of the previous clip's end.
 *  Input must be start-sorted (buildTracks guarantees it). The tolerance is generous (20s) so a
 *  burst of ~40s motion clips separated by a few seconds reads as ONE recording session bar
 *  instead of a row of thin slivers; genuine idle gaps (minutes) still split into separate bars. */
export function mergeSegments(clips: TLClip[], gapToleranceSecs = 20): TLSegment[] {
  const segs: TLSegment[] = [];
  for (const c of clips) {
    const last = segs[segs.length - 1];
    if (last && c.startTs - last.endTs <= gapToleranceSecs * 1000) {
      last.endTs = Math.max(last.endTs, c.endTs);
      last.clipCount += 1;
      last.uploaded = last.uploaded && c.uploaded;
    } else {
      segs.push({ startTs: c.startTs, endTs: c.endTs, clipCount: 1, uploaded: c.uploaded });
    }
  }
  return segs;
}

/** Ruler ticks across `[originMs, endMs]` at the smallest spacing keeping ticks ≥ `minPx` apart. */
export function rulerTicks(
  originMs: number,
  endMs: number,
  pxPerSec: number,
  minPx = 72,
): { x: number; label: string }[] {
  // 1m, 5m, 10m, 15m, 30m, 1h, 2h, 3h, 6h.
  const CANDIDATES = [60, 300, 600, 900, 1800, 3600, 7200, 10800, 21600];
  const stepSec = CANDIDATES.find((s) => s * pxPerSec >= minPx) ?? CANDIDATES[CANDIDATES.length - 1];
  const stepMs = stepSec * 1000;
  const ticks: { x: number; label: string }[] = [];
  const first = Math.ceil(originMs / stepMs) * stepMs;
  for (let t = first; t <= endMs; t += stepMs) {
    ticks.push({ x: xOf(t, originMs, pxPerSec), label: hhmm(t) });
  }
  return ticks;
}

/** `HH:MM` in the viewer's locale. */
export function hhmm(tsMs: number): string {
  const d = new Date(tsMs);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** Zoom presets in px-per-second, coarse → fine. 0.02 ≈ a 24h day in ~1700px. */
export const PX_PER_SEC_PRESETS = [0.02, 0.05, 0.1, 0.25, 0.5] as const;

// ── event marker lane (Phase 2) ─────────────────────────────────────────────

/** One clustered marker: every event that landed in the same time bucket ("same moment"). */
export type TLMarker = {
  ts: number; // bucket centre, epoch ms
  events: EventRow[];
  severity: "info" | "warn" | "error"; // the loudest severity in the bucket
};

/** Severities that outrank each other — the bucket wears its loudest. */
const SEV_RANK: Record<string, number> = { debug: 0, info: 1, warn: 2, error: 3 };

/** Normalise an event severity to the three the lane paints (debug folds into info). */
export function markerSeverity(sev: string): "info" | "warn" | "error" {
  if (sev === "error") return "error";
  if (sev === "warn") return "warn";
  return "info";
}

/** Bucket width in seconds so a cluster spans ~a few px at the current zoom (min 1s). */
export function bucketSecs(pxPerSec: number): number {
  return Math.max(1, Math.round(6 / pxPerSec));
}

/** Cluster events into markers by rounding `occurred_at` to a zoom-dependent bucket. Input order
 *  is preserved within a bucket; markers come out sorted by time. Non-parseable rows are dropped. */
export function buildMarkers(events: EventRow[], pxPerSec: number): TLMarker[] {
  const bMs = bucketSecs(pxPerSec) * 1000;
  const byBucket = new Map<number, EventRow[]>();
  for (const e of events) {
    const t = parseTs(e.occurred_at);
    if (!t) continue;
    const key = Math.round(t / bMs) * bMs;
    const list = byBucket.get(key);
    if (list) list.push(e);
    else byBucket.set(key, [e]);
  }
  return [...byBucket.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([ts, evs]) => ({
      ts,
      events: evs,
      severity: evs.reduce<"info" | "warn" | "error">((acc, e) => {
        const s = markerSeverity(e.severity);
        return SEV_RANK[s] > SEV_RANK[acc] ? s : acc;
      }, "info"),
    }));
}
