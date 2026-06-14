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
    logout();
    void goto("/login");
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

export const admin = {
  tenants: () => api.get<Tenant[]>("/admin/tenants"),
  createTenant: (tenantName: string, adminEmail: string, adminPassword: string) =>
    api.post<string>("/admin/tenants", {
      tenant_name: tenantName,
      admin_email: adminEmail,
      admin_password: adminPassword,
    }),
};
