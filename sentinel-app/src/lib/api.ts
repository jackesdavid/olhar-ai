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
  /** Edge health (reported on the heartbeat): serving | up | offline | disabled. */
  tailscale_state: string | null;
  ffmpeg_ok: boolean | null;
  health_at: string | null;
};

export type NewBox = { id: string; token: string; hostname: string; tailscale_key: string | null };

export type BoxLog = { ts: string; level: string; target: string; message: string };

export const boxes = {
  list: () => api.get<Box[]>("/boxes"),
  create: (name: string) => api.post<NewBox>("/boxes", { name }),
  del: (id: string) => api.del<void>(`/boxes/${id}`),
  /** Queue a restart for a box (it relaunches → reconnects tailnet + re-serves go2rtc). */
  restart: (id: string) => api.post<void>(`/boxes/${id}/restart`),
  /** The latest edge build the operator has published (for "update available"). */
  latestVersion: () => api.get<{ version: string }>("/edge-latest"),
  /** Recent persisted log tail for a box (oldest-first). */
  logs: (id: string, limit = 500) => api.get<BoxLog[]>(`/boxes/${id}/logs?limit=${limit}`),
  /** EventSource URL for a box's live log tail (token in query — EventSource can't set headers). */
  logsStreamUrl: (id: string) =>
    `${API_BASE}/boxes/${encodeURIComponent(id)}/logs/stream?token=${encodeURIComponent(auth.token ?? "")}`,
  patchRecordingConfig: (id: string, config: RecordingConfig) =>
    api.patch<RecordingConfig>(`/boxes/${id}/recording-config`, config),
};

export type RecordingConfig = {
  pre_roll_secs: number;
  max_storage_mb: number;
  codec: string;
};

export type PlatformLog = {
  id: number;
  module: string;
  trace_id: string | null;
  entity_id: string | null;
  entity_type: string | null;
  level: string;
  message: string;
  payload: Record<string, unknown> | null;
  created_at: string;
};

export type PlatformLogsPage = {
  items: PlatformLog[];
  next_cursor: number | null;
  total_in_page: number;
};

export type PlatformLogsFilter = {
  module?: string;
  trace_id?: string;
  entity_type?: string;
  entity_id?: string;
  level?: string;
  from?: string;
  to?: string;
  after?: number;
  limit?: number;
};

export const platformLogs = {
  list: (f: PlatformLogsFilter = {}) => {
    const p = new URLSearchParams();
    if (f.module)      p.set("module",      f.module);
    if (f.trace_id)    p.set("trace_id",    f.trace_id);
    if (f.entity_type) p.set("entity_type", f.entity_type);
    if (f.entity_id)   p.set("entity_id",   f.entity_id);
    if (f.level)       p.set("level",       f.level);
    if (f.from)        p.set("from",        f.from);
    if (f.to)          p.set("to",          f.to);
    if (f.after != null) p.set("after",     String(f.after));
    if (f.limit != null) p.set("limit",     String(f.limit));
    const qs = p.toString();
    return api.get<PlatformLogsPage>(`/platform-logs${qs ? `?${qs}` : ""}`);
  },
};

// ---- Recordings -------------------------------------------------------------

export type Recording = {
  id: number;
  camera_ip: string;
  clip_id: string;
  duration_secs: number;
  size_bytes: number;
  started_at: string;  // RFC3339
  created_at: string;  // RFC3339
};

export const recordings = {
  list: (opts?: { camera_ip?: string; limit?: number }) => {
    const p = new URLSearchParams();
    if (opts?.camera_ip) p.set("camera_ip", opts.camera_ip);
    if (opts?.limit != null) p.set("limit", String(opts.limit));
    const qs = p.toString();
    return api.get<Recording[]>(`/recordings${qs ? `?${qs}` : ""}`);
  },
  del: (id: number) => api.del<void>(`/recordings/${id}`),
  downloadUrl: (id: number) => `${API_BASE}/recordings/${id}/download`,
};

export const admin = {
  tenants: () => api.get<Tenant[]>("/admin/tenants"),
  createTenant: (tenantName: string, adminEmail: string, adminPassword: string) =>
    api.post<string>("/admin/tenants", {
      tenant_name: tenantName,
      admin_email: adminEmail,
      admin_password: adminPassword,
    }),
  getTelegramProvider: () => api.get<import("./types").TelegramProviderState>("/admin/telegram-provider"),
  setTelegramProvider: (botToken: string) =>
    api.put<void>("/admin/telegram-provider", { bot_token: botToken }),
};
