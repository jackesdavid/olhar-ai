// Cloud auth state: the JWT session, the signed-in user's effective permissions, and
// the master "navigate as tenant" swap. Authorization is **action-based** — `hasPerm`
// mirrors the control plane's `Claims::allows` (the master bypasses everything except
// the tenant-private `live.view`).
import { API_BASE } from "./config";

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

export const auth = $state<{ token: string | null; me: Me | null; ready: boolean }>({
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
  if (typeof localStorage === "undefined") return;
  if (t) localStorage.setItem(TOKEN_KEY, t);
  else localStorage.removeItem(TOKEN_KEY);
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
export async function signup(tenantName: string, email: string, password: string): Promise<void> {
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

// ---- master: navigate as tenant ---------------------------------------------

/** Whether the current session is a master acting inside a tenant. */
export function isAssuming(): boolean {
  return typeof localStorage !== "undefined" && !!localStorage.getItem(MASTER_KEY);
}

/** Stash the master token and swap to a tenant-scoped one (acts inside `tenantId`). */
export async function assumeTenant(tenantId: string): Promise<void> {
  const r = await fetch(`${API_BASE}/admin/tenants/${tenantId}/assume`, {
    method: "POST",
    headers: { Authorization: `Bearer ${auth.token}` },
  });
  if (!r.ok) throw new Error((await r.text()) || "Could not assume tenant");
  const data = await r.json();
  if (typeof localStorage !== "undefined" && !localStorage.getItem(MASTER_KEY) && auth.token) {
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
