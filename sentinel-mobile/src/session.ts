// Auth session: the control-plane JWT, kept in the OS keychain/keystore (never AsyncStorage —
// it is a bearer credential). The token is HS256 and shared between control and gateway, so the
// same value authenticates live video later (Phase 2).
import * as SecureStore from "expo-secure-store";
import { CONTROL_BASE } from "./config";

const TOKEN_KEY = "sentinel.token";

export type Session = {
  token: string;
  user_id: string;
  tenant_id: string | null;
  is_master: boolean;
  perms: string[];
};

let token: string | null = null;

// The token is module state, but the route gate in `app/_layout.tsx` has to RE-RUN when it changes —
// reading it once into component state means signing in leaves the gate believing you are signed
// out, and it bounces you straight back to the login screen. So expose it as an external store.
type Listener = () => void;
const listeners = new Set<Listener>();

function setToken(next: string | null) {
  token = next;
  listeners.forEach((l) => l());
}

/** Subscribe to sign-in/sign-out. Pair with `currentToken` via `useSyncExternalStore`. */
export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function currentToken(): string | null {
  return token;
}

/** Restore a previous sign-in on app start. Returns null when there is none. */
export async function restore(): Promise<string | null> {
  setToken(await SecureStore.getItemAsync(TOKEN_KEY));
  return token;
}

export async function signIn(email: string, password: string): Promise<Session> {
  const r = await fetch(`${CONTROL_BASE}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email: email.trim(), password }),
  });
  if (!r.ok) {
    // Control answers 401 with a plain-text reason ("invalid credentials").
    throw new Error((await r.text()) || `sign-in failed (${r.status})`);
  }
  const s = (await r.json()) as Session;
  await SecureStore.setItemAsync(TOKEN_KEY, s.token);
  setToken(s.token);
  return s;
}

export async function signOut(): Promise<void> {
  await SecureStore.deleteItemAsync(TOKEN_KEY);
  setToken(null);
}

/** Authenticated JSON call against the control plane. Throws on non-2xx. */
export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    "content-type": "application/json",
    ...((init.headers as Record<string, string>) ?? {}),
  };
  if (token) headers.authorization = `Bearer ${token}`;
  const r = await fetch(`${CONTROL_BASE}${path}`, { ...init, headers });
  if (r.status === 401) {
    await signOut();
    throw new Error("session expired");
  }
  if (!r.ok) throw new Error((await r.text()) || `${r.status}`);
  if (r.status === 204) return undefined as T;
  return (await r.json()) as T;
}
