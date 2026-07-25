// Where the SvelteKit app reaches the cloud services (control API, streaming gateway).
//
// Resolution is **derived from the current origin**, deliberately NOT a hardcoded domain: the app is
// served at `app.<domain>`, control at `control.<domain>`, gateway at `gateway.<domain>`, so we swap
// the leading label. That is domain-agnostic — it worked for `thesentinels.online` and keeps working
// after a cutover to `olharai.com` with no code change. (A previous version hardcoded `olharai.com`,
// which silently sent the live `app.thesentinels.online` build to the localhost dev fallback — the
// login-calls-127.0.0.1 bug.) An explicit env override still wins for bespoke setups.

/** `control` / `gateway` host for the current origin, e.g. `app.thesentinels.online` →
 *  `https://control.thesentinels.online`. `null` in dev / SSR (no window). */
function cloudHost(service: "control" | "gateway"): string | null {
  if (typeof window === "undefined") return null;
  const h = window.location.hostname;
  if (h === "localhost" || h === "127.0.0.1" || h.endsWith(".local")) return null;
  // `app.` / `www.` prefix → base domain; an apex host maps straight to `<service>.<apex>`.
  const base = h.replace(/^(app|www)\./, "");
  return `https://${service}.${base}`;
}

function resolveBase(): string {
  const env = (import.meta as { env?: Record<string, string> }).env?.VITE_CONTROL_URL;
  if (env) return env.replace(/\/+$/, "");
  return cloudHost("control") ?? "http://127.0.0.1:8790";
}

export const API_BASE = resolveBase();

/** The public streaming gateway base URL (live feeds, `live.view`-gated). */
function resolveGateway(): string {
  const env = (import.meta as { env?: Record<string, string> }).env?.VITE_GATEWAY_URL;
  if (env) return env.replace(/\/+$/, "");
  return cloudHost("gateway") ?? "http://127.0.0.1:8789";
}

export const GATEWAY_BASE = resolveGateway();

/** The deploying commit (short SHA), baked in at build time via `VITE_COMMIT` (CI passes
 * `github.sha`). Shown in the UI footer so a new version is visible at a glance; "dev"
 * for local builds. */
export const COMMIT = ((import.meta as { env?: Record<string, string> }).env?.VITE_COMMIT ?? "dev").slice(0, 7);
