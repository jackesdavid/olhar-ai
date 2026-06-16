// Typed fetch client for the control plane. Injects the JWT bearer, JSON-encodes
// bodies, and on 401 clears the session and bounces to /login. As pages are migrated
// off Tauri `invoke`, they call `api.*` (and the typed `rbac`/`admin` groups) here.
import { goto } from "$app/navigation";

import { API_BASE } from "./config";
import { auth, logout } from "./auth.svelte";

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (auth.token) headers.Authorization = `Bearer ${auth.token}`;
  if (body !== undefined) headers["content-type"] = "application/json";

  const r = await fetch(`${API_BASE}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (r.status === 401) {
    // While on /login the page manages auth itself (an OAuth return sets the token here).
    // A stray unauthenticated call racing that must NOT clear the session or redirect.
    if (typeof window === "undefined" || window.location.pathname !== "/login") {
      logout();
      void goto("/login");
    }
    throw new Error("Session expired — please sign in again.");
  }
  if (!r.ok) {
    const msg = (await r.text()) || `${method} ${path} failed (${r.status})`;
    throw new Error(msg);
  }
  if (r.status === 204) return undefined as T;
  const ct = r.headers.get("content-type") ?? "";
  return (ct.includes("application/json") ? await r.json() : await r.text()) as T;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, body),
  patch: <T>(path: string, body?: unknown) => request<T>("PATCH", path, body),
  del: <T>(path: string) => request<T>("DELETE", path),
};

// ---- RBAC + tenant admin types/helpers --------------------------------------

export type Permission = { key: string; description: string };
export type Role = { id: string; name: string; is_system: boolean };
export type TenantUser = { id: string; email: string; is_master: boolean; role_ids: string[] };
export type Tenant = { id: string; name: string; slug: string; status: string };

export const rbac = {
  permissions: () => api.get<Permission[]>("/permissions"),
  roles: () => api.get<Role[]>("/roles"),
  createRole: (name: string) => api.post<string>("/roles", { name }),
  rolePerms: (id: string) => api.get<string[]>(`/roles/${id}/permissions`),
  setRolePerms: (id: string, keys: string[]) => api.put<void>(`/roles/${id}/permissions`, { keys }),
  users: () => api.get<TenantUser[]>("/users"),
  inviteUser: (email: string, password: string, roleIds: string[]) =>
    api.post<string>("/users", { email, password, role_ids: roleIds }),
  setUserRoles: (id: string, roleIds: string[]) => api.put<void>(`/users/${id}/roles`, { role_ids: roleIds }),
};

export type Box = {
  id: string;
  name: string;
  magicdns_host: string | null;
  version: string | null;
  last_seen: string | null;
  created_at: string;
};

export type NewBox = { id: string; token: string; hostname: string; tailscale_key: string | null };

export type BoxLog = { ts: string; level: string; target: string; message: string };

export const boxes = {
  list: () => api.get<Box[]>("/boxes"),
  create: (name: string) => api.post<NewBox>("/boxes", { name }),
  del: (id: string) => api.del<void>(`/boxes/${id}`),
  /** The latest edge build the operator has published (for "update available"). */
  latestVersion: () => api.get<{ version: string }>("/edge-latest"),
  /** Recent persisted log tail for a box (oldest-first). */
  logs: (id: string, limit = 500) => api.get<BoxLog[]>(`/boxes/${id}/logs?limit=${limit}`),
  /** EventSource URL for a box's live log tail (token in query — EventSource can't set headers). */
  logsStreamUrl: (id: string) =>
    `${API_BASE}/boxes/${encodeURIComponent(id)}/logs/stream?token=${encodeURIComponent(auth.token ?? "")}`,
};

export const admin = {
  tenants: () => api.get<Tenant[]>("/admin/tenants"),
  createTenant: (tenantName: string, adminEmail: string, adminPassword: string) =>
    api.post<string>("/admin/tenants", {
      tenant_name: tenantName,
      admin_email: adminEmail,
      admin_password: adminPassword,
    }),
};
