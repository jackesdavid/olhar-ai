// Joining a tenant from an invite (#181).
//
// Before this, the only way into the app was an email and a password someone had to be told. The
// invite carries both halves of what a phone needs: WHICH cloud to talk to, and proof that this
// person may join.
//
// The cloud URL is the part that is easy to underestimate. `CONTROL_BASE` is a compiled constant,
// and it has already pointed at a host that does not resolve — a build that ships with the wrong
// default reaches nothing and there is no way for the user to fix it. An invite that names its own
// backend makes one binary work against any deployment.
import * as SecureStore from "expo-secure-store";
import { CONTROL_BASE } from "./config";

const BASE_KEY = "sentinel.control_base";

/** The JSON a Settings → Users QR encodes. `v` is there so an old app can refuse a new shape
 *  rather than misread it. */
export type InvitePayload = { v: number; url: string; t: string };

export type InvitePreview = {
  tenant_name: string;
  email: string | null;
  expires_at: string;
};

/** The backend this phone should talk to: whatever an invite set, else the compiled default. */
let overrideBase: string | null = null;

export async function restoreBase(): Promise<void> {
  overrideBase = await SecureStore.getItemAsync(BASE_KEY);
}

export function controlBase(): string {
  return overrideBase || CONTROL_BASE;
}

async function setBase(url: string): Promise<void> {
  const clean = url.replace(/\/+$/, "");
  overrideBase = clean;
  await SecureStore.setItemAsync(BASE_KEY, clean);
}

/** Parse a scanned QR, or `null` if it is not one of ours.
 *
 *  A camera will happily hand us a Wi-Fi QR, a URL, or a shop receipt, so this validates rather
 *  than trusts. The URL must be https for anything but a LAN address: an invite is a credential,
 *  and a QR that could point the app at a plaintext host is a QR that can harvest one.
 */
export function parseInvite(raw: string): InvitePayload | null {
  try {
    const p = JSON.parse(raw) as Partial<InvitePayload>;
    if (typeof p.t !== "string" || !p.t.trim()) return null;
    if (typeof p.url !== "string" || !p.url.trim()) return null;
    if (p.v !== 1) return null;
    const u = new URL(p.url);
    const lan = /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|localhost$|127\.)/.test(u.hostname);
    if (u.protocol !== "https:" && !lan) return null;
    return { v: 1, url: p.url, t: p.t };
  } catch {
    return null;
  }
}

/** What this invite is for — asked BEFORE anything is typed, so the screen can say "Join Acme?".
 *  Also the moment the cloud URL is adopted: if the preview works, that backend is real. */
export async function previewInvite(payload: InvitePayload): Promise<InvitePreview> {
  const base = payload.url.replace(/\/+$/, "");
  const r = await fetch(`${base}/auth/invites/${encodeURIComponent(payload.t)}`);
  if (!r.ok) {
    throw new Error((await r.text()) || "This invite is not valid any more.");
  }
  const preview = (await r.json()) as InvitePreview;
  await setBase(base);
  return preview;
}

/** A typed code instead of a scan — same token, same endpoints, against the CURRENT backend.
 *  Someone typing a code has no way to tell us which cloud, so this only works for the default
 *  (or one a previous invite already set). */
export async function previewCode(code: string): Promise<{ payload: InvitePayload; preview: InvitePreview }> {
  const payload: InvitePayload = { v: 1, url: controlBase(), t: code.trim().toUpperCase() };
  const preview = await previewInvite(payload);
  return { payload, preview };
}

export type Session = {
  token: string;
  user_id: string;
  tenant_id: string | null;
  is_master: boolean;
  perms: string[];
};

/** Redeem it. The server answers with the same session shape `/auth/login` returns, so the app's
 *  existing session handling needs to know nothing about invites. */
export async function acceptInvite(
  payload: InvitePayload,
  password: string,
  email?: string,
): Promise<Session> {
  const base = payload.url.replace(/\/+$/, "");
  const r = await fetch(`${base}/auth/invites/${encodeURIComponent(payload.t)}/accept`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ password, email }),
  });
  if (!r.ok) throw new Error((await r.text()) || "Couldn't accept the invite.");
  await setBase(base);
  return (await r.json()) as Session;
}
