// Where the SvelteKit app reaches the control plane (cloud backend / API).
//
// Resolution order:
//   1. `VITE_CONTROL_URL` build/dev env override (e.g. in `.env`).
//   2. Production: any `*.thesentinels.online` origin → `control.thesentinels.online`.
//   3. Dev fallback: the local control plane on :8790.
function resolveBase(): string {
  const env = (import.meta as { env?: Record<string, string> }).env?.VITE_CONTROL_URL;
  if (env) return env.replace(/\/+$/, "");
  if (typeof window !== "undefined" && window.location.hostname.endsWith("thesentinels.online")) {
    return "https://control.thesentinels.online";
  }
  return "http://127.0.0.1:8790";
}

export const API_BASE = resolveBase();

/** The public streaming gateway base URL (live feeds, `live.view`-gated). */
function resolveGateway(): string {
  const env = (import.meta as { env?: Record<string, string> }).env?.VITE_GATEWAY_URL;
  if (env) return env.replace(/\/+$/, "");
  if (typeof window !== "undefined" && window.location.hostname.endsWith("thesentinels.online")) {
    return "https://gateway.thesentinels.online";
  }
  return "http://127.0.0.1:8789";
}

export const GATEWAY_BASE = resolveGateway();
