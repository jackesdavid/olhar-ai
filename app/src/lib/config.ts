// Where the SvelteKit app reaches the control plane (cloud backend / API).
//
// Resolution order:
//   1. `VITE_CONTROL_URL` build/dev env override (e.g. in `.env`).
//   2. Production: any `*.thesentinels.online` origin → `control.thesentinels.online`.
//   3. Dev fallback: the local control plane on :8790.
//
// The desktop (Tauri) app talks to local Tauri commands via `invoke` and does NOT use
// this — it's only for the browser/cloud client (see `auth.svelte.ts` `isTauri`).
function resolveBase(): string {
  const env = (import.meta as { env?: Record<string, string> }).env?.VITE_CONTROL_URL;
  if (env) return env.replace(/\/+$/, "");
  if (typeof window !== "undefined" && window.location.hostname.endsWith("thesentinels.online")) {
    return "https://control.thesentinels.online";
  }
  return "http://127.0.0.1:8790";
}

export const API_BASE = resolveBase();
