// Global client-error reporter (card #146). Uncaught errors + unhandled promise rejections are
// POSTed to control's `/logs/client` (session-authed), which ships them to Datadog as `service:app`,
// tenant-tagged — so frontend crashes are visible alongside the backend, without a Browser-Logs SDK
// key in the bundle. Fire-and-forget + rate-limited + deduped: reporting must never itself throw,
// loop, or flood.
import { API_BASE } from "./config";
import { auth } from "./auth.svelte";

const APP_VERSION = "0.4.1"; // mirrors package.json; stamped on every report for build correlation

// Backpressure: at most N reports/window, and never the same signature twice within it — a render
// loop that throws every frame must not DDoS control or blow the Datadog budget.
const MAX_PER_WINDOW = 20;
const WINDOW_MS = 60_000;
let windowStart = 0;
let sentInWindow = 0;
const recentSigs = new Set<string>();

function allow(sig: string): boolean {
  const now = Date.now();
  if (now - windowStart > WINDOW_MS) {
    windowStart = now;
    sentInWindow = 0;
    recentSigs.clear();
  }
  if (recentSigs.has(sig)) return false;
  if (sentInWindow >= MAX_PER_WINDOW) return false;
  recentSigs.add(sig);
  sentInWindow += 1;
  return true;
}

type Report = {
  level?: string;
  message: string;
  stack?: string;
  source?: string;
  line?: number;
  col?: number;
  kind?: string;
};

/** Best-effort ship one report. Silent on every failure; skips when signed out. */
export function reportClientError(r: Report): void {
  if (typeof window === "undefined" || !auth.token) return; // session-authed only
  const sig = `${r.kind ?? "error"}:${r.message}`.slice(0, 300);
  if (!allow(sig)) return;

  const body = JSON.stringify({
    level: r.level ?? "error",
    message: r.message?.slice(0, 2000) ?? "client error",
    stack: r.stack?.slice(0, 6000),
    source: r.source,
    line: r.line,
    col: r.col,
    kind: r.kind ?? "error",
    url: window.location?.href,
    app_version: APP_VERSION,
  });
  try {
    void fetch(`${API_BASE}/logs/client`, {
      method: "POST",
      headers: { "content-type": "application/json", Authorization: `Bearer ${auth.token}` },
      body,
      keepalive: true, // still delivers if the page is unloading (e.g. a crash on navigation)
    }).catch(() => {});
  } catch {
    // never let telemetry throw
  }
}

let installed = false;

/** Attach the global handlers once (browser-only). Called from `hooks.client.ts` at app start. */
export function installErrorReporter(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;

  window.addEventListener("error", (e: ErrorEvent) => {
    reportClientError({
      kind: "onerror",
      message: e.message || String(e.error ?? "error"),
      stack: e.error?.stack,
      source: e.filename,
      line: e.lineno,
      col: e.colno,
    });
  });

  window.addEventListener("unhandledrejection", (e: PromiseRejectionEvent) => {
    const reason = e.reason;
    reportClientError({
      kind: "unhandledrejection",
      message: reason?.message ?? String(reason ?? "unhandled rejection"),
      stack: reason?.stack,
    });
  });
}
