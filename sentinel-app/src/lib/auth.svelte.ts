// Cloud auth state: the JWT session, the signed-in user's effective permissions, and
// the master "navigate as tenant" swap. Authorization is **action-based** — `hasPerm`
// mirrors the control plane's `Claims::allows` (the master bypasses everything except
// the tenant-private `live.view`).
import { API_BASE } from "./config";
import {
  startRegistration,
  startAuthentication,
  browserSupportsWebAuthn,
} from "@simplewebauthn/browser";

export type Me = {
  user_id: string;
  tenant_id: string | null;
  is_master: boolean;
  act_as: boolean;
  perms: string[];
  email?: string;
};

const TOKEN_KEY = "sentinel.token";
/** While the master is "acting as" a tenant, their original token is stashed here. */
const MASTER_KEY = "sentinel.masterToken";

export const auth = $state<{
  token: string | null;
  me: Me | null;
  ready: boolean;
}>({
  token: null,
  me: null,
  ready: false,
});

/** Load the persisted token into state (call once on boot). */
export function loadToken(): void {
  if (typeof localStorage === "undefined") return;
  auth.token = localStorage.getItem(TOKEN_KEY);
}

export function setToken(t: string | null): void {
  auth.token = t;
  // The media token is derived from the session, so it never outlives one. Dropping it here is
  // what makes sign-out and "act as tenant" safe — a stale media token from the previous identity
  // would otherwise keep working until it expired.
  media = null;
  if (t) void primeMediaToken();
  if (typeof localStorage === "undefined") return;
  if (t) localStorage.setItem(TOKEN_KEY, t);
  else localStorage.removeItem(TOKEN_KEY);
}

// ---- media tokens (security plan §1.8) --------------------------------------
// `<img src>` and `EventSource` cannot send an Authorization header, so those URLs carry the token
// in the query string — where it ends up in browser history, `Referer`, and proxy access logs. A
// media token is the same identity narrowed to `scope: "media"`: control refuses it on every
// ordinary API endpoint, so one that leaks cannot be used to call anything, and it expires in an
// hour instead of a day.

let media: { token: string; expiresAt: number } | null = null;
let mediaInflight: Promise<void> | null = null;

/**
 * Mint (or re-mint) the media token. Cheap to call often: it no-ops while the current one is still
 * comfortably fresh, and collapses concurrent callers onto one request.
 */
export async function primeMediaToken(): Promise<void> {
  if (!auth.token) {
    media = null;
    return;
  }
  // Re-mint with five minutes to spare, so a URL built now is still valid when the image actually
  // loads. Expiring mid-flight would look like a broken thumbnail, not an auth problem.
  if (media && media.expiresAt - Date.now() > 5 * 60_000) return;
  if (mediaInflight) return mediaInflight;
  mediaInflight = (async () => {
    try {
      const r = await fetch(`${API_BASE}/auth/media-token`, {
        method: "POST",
        headers: { Authorization: `Bearer ${auth.token}` },
      });
      if (!r.ok) return;
      const body = (await r.json()) as { token: string; expires_in: number };
      media = {
        token: body.token,
        expiresAt: Date.now() + body.expires_in * 1000,
      };
    } catch {
      // Deliberately silent: phase 1 of the rollout still accepts session tokens on media
      // surfaces, so a failed mint degrades to the old behaviour instead of breaking every image.
    } finally {
      mediaInflight = null;
    }
  })();
  return mediaInflight;
}

/**
 * The token to put in a media URL. Prefers the media token, falling back to the session token when
 * one hasn't been minted yet (first paint, or a failed mint) — which control still accepts during
 * phase 1. Synchronous on purpose: these URLs are built inline in `src={...}`.
 */
export function mediaToken(): string | null {
  void primeMediaToken(); // keep it warm; fire-and-forget
  if (media && media.expiresAt > Date.now()) return media.token;
  return auth.token;
}

/** Whether the signed-in user may perform `action`. Action-based — never role names. */
export function hasPerm(action: string): boolean {
  const me = auth.me;
  if (!me) return false;
  if (me.perms.includes(action)) return true;
  return me.is_master && action !== "live.view";
}

/** Fetch `/auth/me` for the current token. Returns false (and clears `me`) on failure. */
export async function loadMe(): Promise<boolean> {
  if (!auth.token) {
    auth.me = null;
    return false;
  }
  try {
    const r = await fetch(`${API_BASE}/auth/me`, {
      headers: { Authorization: `Bearer ${auth.token}` },
    });
    if (!r.ok) {
      auth.me = null;
      return false;
    }
    auth.me = (await r.json()) as Me;
    // Warm the media token on boot, so the first thumbnail already has one rather than falling back
    // to the session token for the first paint.
    void primeMediaToken();
    return true;
  } catch {
    auth.me = null;
    return false;
  }
}

export async function login(email: string, password: string): Promise<void> {
  const r = await fetch(`${API_BASE}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  if (!r.ok) throw new Error((await r.text()) || "Login failed");
  const data = await r.json();
  setToken(data.token);
  await loadMe();
}

/** Public signup — creates a new tenant and makes this user its Admin. */
export async function signup(
  tenantName: string,
  email: string,
  password: string,
): Promise<void> {
  const r = await fetch(`${API_BASE}/auth/signup`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ tenant_name: tenantName, email, password }),
  });
  if (!r.ok) throw new Error((await r.text()) || "Sign up failed");
  const data = await r.json();
  setToken(data.token);
  await loadMe();
}

export function logout(): void {
  setToken(null);
  if (typeof localStorage !== "undefined") localStorage.removeItem(MASTER_KEY);
  auth.me = null;
}

/** Begin Google login (full-page redirect to the control plane's start endpoint). */
export function googleLoginUrl(): string {
  return `${API_BASE}/auth/oauth/google/start`;
}

// ---- FIDO2 / WebAuthn passkeys (card #147, §2 E0) ----------------------------

/** Whether this browser can do WebAuthn at all (hide the passkey UI otherwise). */
export function passkeysSupported(): boolean {
  return browserSupportsWebAuthn();
}

/** Register a new passkey for the signed-in user (a roaming key or this device's biometric). */
export async function registerPasskey(nickname?: string): Promise<void> {
  const start = await fetch(`${API_BASE}/auth/webauthn/register/start`, {
    method: "POST",
    headers: { Authorization: `Bearer ${auth.token}` },
  });
  if (!start.ok)
    throw new Error((await start.text()) || "Could not start registration");
  const optionsJSON = (await start.json()).publicKey;
  const credential = await startRegistration({ optionsJSON });
  const finish = await fetch(`${API_BASE}/auth/webauthn/register/finish`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      Authorization: `Bearer ${auth.token}`,
    },
    body: JSON.stringify({ credential, nickname: nickname?.trim() || null }),
  });
  if (!finish.ok)
    throw new Error((await finish.text()) || "Could not save the passkey");
}

/** Sign in with a passkey. Returns false when this address has no passkey (fall back to password). */
export async function loginWithPasskey(email: string): Promise<boolean> {
  const start = await fetch(`${API_BASE}/auth/webauthn/login/start`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email }),
  });
  if (start.status === 404) return false;
  if (!start.ok)
    throw new Error((await start.text()) || "Passkey login failed");
  const { handle, options } = await start.json();
  const credential = await startAuthentication({
    optionsJSON: options.publicKey,
  });
  const finish = await fetch(`${API_BASE}/auth/webauthn/login/finish`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ handle, credential }),
  });
  if (!finish.ok)
    throw new Error((await finish.text()) || "Passkey login failed");
  const data = await finish.json();
  setToken(data.token);
  await loadMe();
  return true;
}

export type PasskeyMeta = {
  id: string;
  nickname: string | null;
  created_at: string;
  last_used_at: string | null;
};

/** The signed-in user's registered passkeys. */
export async function listPasskeys(): Promise<PasskeyMeta[]> {
  const r = await fetch(`${API_BASE}/auth/webauthn/credentials`, {
    headers: { Authorization: `Bearer ${auth.token}` },
  });
  if (!r.ok) throw new Error("Could not load passkeys");
  return r.json();
}

/** Remove one of the signed-in user's passkeys. */
export async function deletePasskey(id: string): Promise<void> {
  const r = await fetch(`${API_BASE}/auth/webauthn/credentials/${id}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${auth.token}` },
  });
  if (!r.ok) throw new Error("Could not remove the passkey");
}

// ---- master: navigate as tenant ---------------------------------------------

/** Whether the current session is a master acting inside a tenant. */
export function isAssuming(): boolean {
  return (
    typeof localStorage !== "undefined" && !!localStorage.getItem(MASTER_KEY)
  );
}

/** Stash the master token and swap to a tenant-scoped one (acts inside `tenantId`). */
export async function assumeTenant(tenantId: string): Promise<void> {
  const r = await fetch(`${API_BASE}/admin/tenants/${tenantId}/assume`, {
    method: "POST",
    headers: { Authorization: `Bearer ${auth.token}` },
  });
  if (!r.ok) throw new Error((await r.text()) || "Could not assume tenant");
  const data = await r.json();
  if (
    typeof localStorage !== "undefined" &&
    !localStorage.getItem(MASTER_KEY) &&
    auth.token
  ) {
    localStorage.setItem(MASTER_KEY, auth.token);
  }
  setToken(data.token);
  await loadMe();
}

/** Restore the master's original token (exit "acting as"). */
export async function exitAssume(): Promise<void> {
  if (typeof localStorage === "undefined") return;
  const master = localStorage.getItem(MASTER_KEY);
  if (master) {
    localStorage.removeItem(MASTER_KEY);
    setToken(master);
    await loadMe();
  }
}
