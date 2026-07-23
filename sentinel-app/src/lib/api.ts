// Typed fetch client for the control plane. Injects the JWT bearer, JSON-encodes
// bodies, and on 401 clears the session and bounces to /login. As pages are migrated
// off Tauri `invoke`, they call `api.*` (and the typed `rbac`/`admin` groups) here.
import { goto } from "$app/navigation";

import { API_BASE } from "./config";
import { auth, logout } from "./auth.svelte";
import { lock } from "./billing.svelte";

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
  // Soft-lock: the tenant's subscription lapsed, so writes are refused but reads still
  // work. Record it so the layout can show a banner, not just this one failed call.
  if (r.status === 402) {
    const msg = (await r.text()) || "Your subscription needs attention.";
    lock(msg);
    throw new Error(msg);
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
  /** Whether gst-launch-1.0 is on PATH (card #138) — null until an updated edge reports it. */
  gstreamer_ok: boolean | null;
  health_at: string | null;
  recording_config: RecordingConfig | null;
  /** Reported platform, e.g. "macOS (aarch64)" / "Bazzite (x86_64)" (heartbeat). */
  platform: string | null;
  /** Working GPU video-decode method ("vaapi"/"qsv"/"cuda"/"videotoolbox"/"v4l2m2m") or "none" (#120). */
  hwaccel: string | null;
  /** Resource usage (percent 0..100), reported on the heartbeat. `gpu_*` null without an NVIDIA GPU. */
  cpu_pct: number | null;
  mem_pct: number | null;
  disk_pct: number | null;
  gpu_pct: number | null;
  gpu_mem_pct: number | null;
  metrics_at: string | null;
};

export type NewBox = { id: string; token: string; hostname: string; tailscale_key: string | null };

export type InstallerOS = "macos" | "windows" | "linux";
export type Installer = {
  os: InstallerOS;
  label: string;
  kind: "pkg" | "zip" | "sh";
  /** Path under the control host (e.g. "/edge/Sentinel-Edge-0.5.39.pkg"); prefix with API_BASE. */
  url: string;
  size_bytes: number | null;
};
export type InstallerManifest = { version: string; installers: Installer[] };

export type BoxLog = { ts: string; level: string; target: string; message: string };

export const boxes = {
  list: () => api.get<Box[]>("/boxes"),
  create: (name: string) => api.post<NewBox>("/boxes", { name }),
  del: (id: string) => api.del<void>(`/boxes/${id}`),
  /** Queue a restart for a box (it relaunches → reconnects tailnet + re-serves go2rtc). */
  restart: (id: string) => api.post<void>(`/boxes/${id}/restart`),
  /** Queue an update for a box (it runs install.sh --update → service manager restarts with new binary). */
  update: (id: string) => api.post<void>(`/boxes/${id}/update`),
  /** The latest edge build the operator has published (for "update available"). */
  latestVersion: () => api.get<{ version: string }>("/edge-latest"),
  /** Per-platform installer downloads for the current edge version (card #84). Only lists files
   *  that actually exist on the dist server, so a button never 404s. */
  installers: () => api.get<InstallerManifest>("/edge/installers"),
  /**
   * Mint a pairing code for a box (card #81). The person at the appliance types it into the
   * enrollment page the installer opens; redeeming it issues the box's real token, so this code —
   * not the token — is the only thing that needs to travel.
   */
  pairingCode: (id: string) => api.post<{ code: string; expires_at: string }>(`/boxes/${id}/pairing-code`, {}),
  /** Pairing progress for the install view (card #85): `claimed` = the code was redeemed, `online`
   *  = the box has checked in since. Lets the UI show "code accepted, starting up…" before online. */
  pairingStatus: (id: string) => api.get<{ claimed: boolean; online: boolean }>(`/boxes/${id}/pairing-status`),
  /** Recent persisted log tail for a box (oldest-first). */
  logs: (id: string, limit = 500) => api.get<BoxLog[]>(`/boxes/${id}/logs?limit=${limit}`),
  /** EventSource URL for a box's live log tail (token in query — EventSource can't set headers). */
  logsStreamUrl: (id: string) =>
    `${API_BASE}/boxes/${encodeURIComponent(id)}/logs/stream?token=${encodeURIComponent(auth.token ?? "")}`,
  // Partial, not RecordingConfig: the edge fills in any missing key with its own default
  // (serde `#[serde(default)]` per field), so a caller that only wants to change ONE knob (e.g.
  // media_backend, card #138) can spread the box's existing `recording_config` and override just
  // that key, without having to reconstruct the full shape.
  patchRecordingConfig: (id: string, config: Partial<RecordingConfig>) =>
    api.patch<RecordingConfig>(`/boxes/${id}/recording-config`, config),
  storageStats: (id: string) => api.get<StorageStats>(`/boxes/${id}/storage-stats`),
};

export type RecordingConfig = {
  pre_roll_secs: number;
  max_storage_mb: number;
  recordings_path: string;
  codec: string;
  cameras_always_warm: boolean;
  /** Which stream feeds face capture (#120): "preview" (substream) or "hd" (main stream). */
  face_capture_stream?: "preview" | "hd";
  motion_var_threshold?: number;
  /** Drop a clip's local file once the cloud confirms it's durable (the box becomes a spool). */
  delete_local_after_upload?: boolean;
  /** Which media tool this box shells out to for decode/mux (card #138): "ffmpeg" (default) or
   *  "gstreamer" — a per-box coexistence/measurement flag, not a permanent switch. See
   *  docs/plans/edge-gstreamer-backend.md. */
  media_backend?: "ffmpeg" | "gstreamer";
};

export type StorageStats = {
  used_bytes: number;
  max_bytes: number;
  clip_count: number;
  path: string;
  pct: number;
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

/** Where a clip lives. `pending` = edge only, `uploading` = in transit, `uploaded` = durable
 *  in the cloud, `failed` = upload gave up. Drives the badge + whether cloud playback is offered. */
export type UploadState = "pending" | "uploading" | "uploaded" | "failed";

export type Recording = {
  id: number;
  camera_ip: string;
  camera_name?: string | null;
  clip_id: string;
  friendly_name: string;
  duration_secs: number;
  size_bytes: number;
  started_at: string;  // RFC3339
  created_at: string;  // RFC3339
  upload_state: UploadState;
  /** A delete request is still working through the queue for this clip; it vanishes once done. */
  deleting: boolean;
};

export type RecordingPage = {
  total: number;
  items: Recording[];
};

export type RecordingListOpts = {
  camera_ip?: string;
  from?: string;
  to?: string;
  sort?: string;
  dir?: "asc" | "desc";
  limit?: number;
  offset?: number;
};

export type RecordingCamera = { ip: string; name: string };

export const recordings = {
  list: (opts?: RecordingListOpts) => {
    const p = new URLSearchParams();
    if (opts?.camera_ip) p.set("camera_ip", opts.camera_ip);
    if (opts?.from)      p.set("from", opts.from);
    if (opts?.to)        p.set("to", opts.to);
    if (opts?.sort)      p.set("sort", opts.sort);
    if (opts?.dir)       p.set("dir", opts.dir);
    if (opts?.limit != null)  p.set("limit",  String(opts.limit));
    if (opts?.offset != null) p.set("offset", String(opts.offset));
    const qs = p.toString();
    return api.get<RecordingPage>(`/recordings${qs ? `?${qs}` : ""}`);
  },
  cameras: () => api.get<RecordingCamera[]>("/recordings/cameras"),
  /** Presigned R2 URL to stream a cloud clip. 409 if the clip isn't uploaded (play the edge copy). */
  cloudUrl: (id: number) => api.get<{ url: string }>(`/recordings/${id}/url`),
  del: (id: number) => api.del<void>(`/recordings/${id}`),
  bulkDel: (ids: number[]) =>
    api.post<{ deleted: number }>("/recordings/bulk-delete", { ids }),
  /** Fetch the clip with the JWT bearer and return a Blob for client-side save. */
  download: async (id: number): Promise<Blob> => {
    const headers: Record<string, string> = {};
    if (auth.token) headers.Authorization = `Bearer ${auth.token}`;
    const r = await fetch(`${API_BASE}/recordings/${id}/download`, { headers });
    if (!r.ok) throw new Error(`Download failed (${r.status})`);
    return r.blob();
  },
};

// ---- Master: async work-queue load (all tenants or one) ---------------------

/** A count of jobs in one (category, status) — the load overview aggregates these. */
export type CategoryStat = { category: string; status: string; count: number };
/** One job row for the load detail table (no payload — this is a volume view). */
export type MonitorJob = {
  id: string;
  tenant_id: string;
  category: string;
  kind: string;
  status: string;
  attempts: number;
  created_at: string;
  updated_at: string;
  last_error: string | null;
};
export type JobsPage = { total: number; items: MonitorJob[] };
export type JobsListOpts = { tenant?: string; category?: string; status?: string; window?: number; limit?: number; offset?: number };

/** Jobs created per time bucket within the window. `t` = bucket-start unix epoch (seconds). */
export type JobSeries = { bucket_secs: number; points: { t: number; c: number }[] };

export const adminJobs = {
  /** Job counts by (category, status) within `window` seconds; omit tenant for the whole system. */
  stats: (tenant?: string, window?: number) => {
    const p = new URLSearchParams();
    if (tenant) p.set("tenant", tenant);
    if (window != null) p.set("window", String(window));
    const qs = p.toString();
    return api.get<CategoryStat[]>(`/admin/jobs/stats${qs ? `?${qs}` : ""}`);
  },
  /** Time series of jobs created per bucket, for the load-over-time chart. */
  series: (tenant?: string, window?: number) => {
    const p = new URLSearchParams();
    if (tenant) p.set("tenant", tenant);
    if (window != null) p.set("window", String(window));
    const qs = p.toString();
    return api.get<JobSeries>(`/admin/jobs/series${qs ? `?${qs}` : ""}`);
  },
  list: (opts?: JobsListOpts) => {
    const p = new URLSearchParams();
    if (opts?.tenant) p.set("tenant", opts.tenant);
    if (opts?.category) p.set("category", opts.category);
    if (opts?.status) p.set("status", opts.status);
    if (opts?.window != null) p.set("window", String(opts.window));
    if (opts?.limit != null) p.set("limit", String(opts.limit));
    if (opts?.offset != null) p.set("offset", String(opts.offset));
    const qs = p.toString();
    return api.get<JobsPage>(`/admin/jobs${qs ? `?${qs}` : ""}`);
  },
};

// ---- Events (agnostic event stream, card #66) -------------------------------

export type EventRow = {
  id: number;
  kind: string;
  source: string;
  severity: string;
  title: string | null;
  message: string | null;
  camera: string | null;
  clip_id: string | null;
  room_id: number | null;
  room_name: string | null;
  device_id: string | null;
  graph_id: number | null;
  user_id: string | null;
  data: unknown;
  occurred_at: string; // RFC3339
  created_at: string;  // RFC3339
};
export type EventKind = { kind: string; label: string };
export type EventListOpts = {
  kind?: string; source?: string; severity?: string; camera?: string;
  from?: string; to?: string; after?: number; limit?: number;
};

export const events = {
  list: (opts?: EventListOpts) => {
    const p = new URLSearchParams();
    if (opts?.kind) p.set("kind", opts.kind);
    if (opts?.source) p.set("source", opts.source);
    if (opts?.severity) p.set("severity", opts.severity);
    if (opts?.camera) p.set("camera", opts.camera);
    if (opts?.from) p.set("from", opts.from);
    if (opts?.to) p.set("to", opts.to);
    if (opts?.after != null) p.set("after", String(opts.after));
    if (opts?.limit != null) p.set("limit", String(opts.limit));
    const qs = p.toString();
    return api.get<EventRow[]>(`/events${qs ? `?${qs}` : ""}`);
  },
  kinds: () => api.get<EventKind[]>("/events/kinds"),
  // Alert-kind events still route through the /alerts endpoints — a thin view over `events`
  // (the alert *model* is gone; these are event operations kept under the historical path):
  // deliver an alert event to the beacons, delete any event, or raise one by hand.
  deliver: (id: number) => api.post<string>(`/alerts/${id}/deliver`),
  remove: (id: number) => api.del<void>(`/alerts/${id}`),
  create: (level: string, message: string, room?: string | null, source?: string | null) =>
    api.post<EventRow>("/alerts", { level, message, room, source }),
};

// ---- Assistant (chat + voice) -----------------------------------------------

export type ChatTurn = { role: "user" | "assistant"; content: string };

/** A tool side-effect rendered inline in the assistant's reply. `type` discriminates. */
export type ChatToolEvent =
  | { type: "snapshot"; camera: string; image: string }
  | { type: "image"; label: string; image: string }
  | { type: "open_live"; ip: string }
  | { type: "devices_changed" }
  | { type: "alerts_changed" }
  | { type: "print"; message: string; level: string };

export type ChatReply = { text: string; tool_events: ChatToolEvent[] };

export const agent = {
  /** Run one conversation turn; send the full transcript (stateless server). */
  chat: (history: ChatTurn[]) => api.post<ChatReply>("/agent/chat", { history }),
  /** Synthesize a reply to speech; returns an MP3 blob (503 if voice isn't configured). */
  tts: async (text: string): Promise<Blob | null> => {
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (auth.token) headers.Authorization = `Bearer ${auth.token}`;
    const r = await fetch(`${API_BASE}/agent/tts`, { method: "POST", headers, body: JSON.stringify({ text }) });
    if (r.status === 503) return null; // voice disabled — degrade to silence
    if (!r.ok) throw new Error(`TTS failed (${r.status})`);
    return r.blob();
  },
  /** Transcribe a recorded clip; posts the raw MediaRecorder blob with its mime type. */
  stt: async (blob: Blob): Promise<string> => {
    const headers: Record<string, string> = { "content-type": blob.type || "audio/webm" };
    if (auth.token) headers.Authorization = `Bearer ${auth.token}`;
    const r = await fetch(`${API_BASE}/agent/stt`, { method: "POST", headers, body: blob });
    if (!r.ok) throw new Error(`Transcription failed (${r.status})`);
    return ((await r.json()) as { text: string }).text;
  },
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
    api.put<import("./types").TelegramProviderState>("/admin/telegram-provider", { bot_token: botToken }),
};

// ---- Billing (#63) ----------------------------------------------------------

/** A subscription tier (master catalog) — money lives in per-currency prices. */
export type PlanTier = {
  id: string;
  name: string;
  camera_cap: number;
  trial_days: number | null;
  /** Days of cloud recording retention this tier sells. 0 = no cloud recording. */
  retention_days: number;
  included_gpu_sec: number;
  included_storage_gb: number;
  included_notifications: number;
  active: boolean;
};
/** Money is per-currency; quotas live on the tier. */
export type PlanMoney = {
  currency: string;
  amount: number;
  overage_gpu_per_sec: number;
  overage_storage_per_gb: number;
  overage_notification_each: number;
};
/** A tier priced in one currency — what tenants see. */
export type PricedPlan = PlanTier & PlanMoney;
export type PlanPrice = { plan_id: string; currency: string; amount: number; overage_storage_per_gb: number };
/** A tier plus every currency it's priced in (master catalog). */
export type PlanWithPrices = PlanTier & { prices: PlanPrice[] };

export type BillingSubscription = {
  id: string;
  tenant_id: string;
  plan_id: string;
  status: string;
  started_at: string;
  ended_at: string | null;
};
export type Invoice = {
  id: string;
  tenant_id: string;
  period: string;
  plan_id: string;
  plan_name: string;
  currency: string;
  base_fee: number;
  fx_usd_brl: number | null;
  total: number;
  status: string;
  created_at: string;
};
export type InvoiceLineItem = {
  driver: string;
  qty: number;
  unit: string;
  included: number;
  billable_qty: number;
  rate: number;
  amount: number;
};
export type InvoiceDraft = {
  plan_name: string;
  currency: string;
  base_fee: number;
  total: number;
  line_items: InvoiceLineItem[];
};
export type InvoiceView = {
  invoice: Invoice | null;
  line_items: InvoiceLineItem[];
  draft: InvoiceDraft | null;
};
export type TenantBilling = {
  tenant_id: string;
  tenant_name: string;
  currency: string;
  plan_id: string | null;
  plan_name: string | null;
  base_fee: number | null;
  status: string | null;
  ended_at: string | null;
};
export type SubscribeBody = {
  plan_id: string;
  billing_type: "PIX" | "BOLETO" | "CREDIT_CARD";
  name: string;
  email: string;
  cpf_cnpj?: string;
};

/** Tenant self-serve billing. Plans come priced in the tenant's own currency. */
/** One tenant's storage picture: durable footage now (stock) + the billed GB-month + quota. */
export type TenantStorage = {
  tenant_id: string;
  tenant_name: string;
  currency: string;
  plan_id: string | null;
  plan_name: string | null;
  retention_days: number;
  included_storage_gb: number;
  overage_per_gb: number;
  /** Durable footage right now — what the customer sees "used". */
  stored_bytes: number;
  clip_count: number;
  /** Average GB across the period's sampled days — the billed quantity. 0 until the first sample. */
  gb_month: number;
  projected_overage: number;
};

export const billing = {
  plans: () => api.get<PricedPlan[]>("/billing/plans"),
  /** This tenant's storage usage vs quota. Null when the tenant has never stored a clip. */
  storage: () => api.get<TenantStorage | null>("/billing/storage"),
  /** Billing types the configured gateway can actually charge. Empty = subscribing unavailable. */
  paymentMethods: () => api.get<SubscribeBody["billing_type"][]>("/billing/payment-methods"),
  subscription: () => api.get<BillingSubscription | null>("/billing/subscription"),
  invoices: () => api.get<Invoice[]>("/billing/invoices"),
  invoice: (period: string) => api.get<InvoiceView>(`/billing/invoices/${period}`),
  subscribe: (body: SubscribeBody) =>
    api.post<{ subscription_ref: string | null; payment_url: string | null }>("/billing/subscribe", body),
};

/** Master cross-tenant billing + plan administration. */
export const adminBilling = {
  overview: () => api.get<TenantBilling[]>("/admin/billing/overview"),
  /** Every tenant's storage, heaviest GB-month first — catches a runaway recorder. */
  storage: () => api.get<TenantStorage[]>("/admin/storage"),
  /** Delete a tenant's cloud footage now (leaving for good / data-subject request). Audited. */
  purgeStorage: (tenant: string) =>
    api.post<{ objects_removed: number; rows_cleared: number }>(`/admin/storage/${tenant}/purge`),
  invoices: (period: string) => api.get<Invoice[]>(`/admin/billing?period=${period}`),
  close: (tenant: string, period: string) => api.post<Invoice>(`/admin/billing/${tenant}/close?period=${period}`),
  grantTrial: (tenant: string) => api.post<{ subscription_id: string }>(`/admin/billing/${tenant}/trial`),
  setPlan: (tenant: string, planId: string) =>
    api.post<{ subscription_id: string }>(`/admin/billing/${tenant}/plan`, { plan_id: planId }),
  plans: () => api.get<PlanWithPrices[]>("/admin/plans"),
  upsertPlan: (plan: PlanTier) => api.post<void>("/admin/plans", plan),
  setPrice: (planId: string, currency: string, amount: number, overageStoragePerGb = 0) =>
    api.post<void>(`/admin/plans/${planId}/price`, {
      currency,
      amount,
      overage_storage_per_gb: overageStoragePerGb,
    }),
};
