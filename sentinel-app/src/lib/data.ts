// Data layer: one API the pages call, dispatching to the control-plane HTTP API. Sentinel
// is web-only now (the Tauri desktop shell was removed), so every command goes to the cloud
// backend (`api.*`) or localStorage for UI-local state. A few capabilities have no cloud
// backend yet (ONVIF discovery, credential test, snapshots, chat/voice, beacon test) and
// throw a clear error via `cloudOnly(...)` — pages don't surface those affordances.
import { api } from "./api";
import { API_BASE } from "./config";
import { auth } from "./auth.svelte";
import {
  DEFAULT_SETTINGS,
  type Alert,
  type AlertStats,
  type Device,
  type MapDoc,
  type Presence,
  type NotificationTarget,
  type Person,
  type PersonEmbedding,
  type RecognizeDetail,
  type Settings,
  type TelegramConfigState,
} from "./types";

/** Unsubscribe handle returned by [`onEvent`]. */
export type UnlistenFn = () => void;

/** A camera the tenant's edge found on its LAN (pushed up to the cloud). */
export type EdgeDiscovered = {
  ip: string;
  name: string;
  xaddr: string;
  manufacturer: string | null;
  model: string | null;
  onvif_id: string | null;
  last_seen: string;
};

const SETTINGS_KEY = "sentinel.settings";

function cloudOnly(feature: string): never {
  throw new Error(`${feature} is not available in the web app yet.`);
}

// ---- live push: one shared SSE stream from the control relay ------------------
// The browser opens a SINGLE `EventSource` to control's `/workflows/events` (which relays
// sentinel-flow's run progress + alert/device pokes, tenant-scoped) and demultiplexes it by
// the event's `event` field to the per-name subscribers below.
const cloudSubs = new Map<string, Set<(payload?: unknown) => void>>();
let cloudStream: EventSource | null = null;

function ensureCloudStream(): void {
  if (cloudStream || typeof EventSource === "undefined" || !auth.token) return;
  const url = `${API_BASE}/workflows/events?token=${encodeURIComponent(auth.token)}`;
  const es = new EventSource(url);
  // Fired on first connect AND on every auto-reconnect. Subscribers to the synthetic
  // "stream-open" event use it to re-seed state that arrives only as deltas (e.g. run
  // activity), since deltas emitted while disconnected are missed.
  es.onopen = () => {
    cloudSubs.get("stream-open")?.forEach((cb) => cb());
  };
  es.onmessage = (e) => {
    let msg: { event?: string };
    try {
      msg = JSON.parse(e.data);
    } catch {
      return;
    }
    if (!msg.event) return;
    cloudSubs.get(msg.event)?.forEach((cb) => cb(msg));
  };
  es.onerror = (e) => {
    const src = e.target as EventSource;
    // CONNECTING (0) = browser is retrying on its own — leave it alone.
    // CLOSED (2) = browser gave up (e.g. 502 during pod restart) — clear and retry manually.
    if (src.readyState === EventSource.CLOSED) {
      cloudSubs.get("stream-closed")?.forEach((cb) => cb());
      if (cloudStream === src) cloudStream = null;
      setTimeout(ensureCloudStream, 3000);
    }
  };
  cloudStream = es;
}

function closeCloudStreamIfIdle(): void {
  if (cloudStream && [...cloudSubs.values()].every((s) => s.size === 0)) {
    cloudStream.close();
    cloudStream = null;
  }
}

/** Subscribe to a backend push event over the relayed SSE stream (control → sentinel-flow).
 * Returns an unsubscribe function. */
export async function onEvent(name: string, cb: (payload?: unknown) => void): Promise<UnlistenFn> {
  let set = cloudSubs.get(name);
  if (!set) {
    set = new Set();
    cloudSubs.set(name, set);
  }
  set.add(cb);
  ensureCloudStream();
  return () => {
    cloudSubs.get(name)?.delete(cb);
    closeCloudStreamIfIdle();
  };
}

export const data = {
  // ---- settings (UI-local: localStorage) ----------------------------------
  getSettings: async (): Promise<Settings> => {
    try {
      const raw = localStorage.getItem(SETTINGS_KEY);
      return raw ? { ...DEFAULT_SETTINGS, ...JSON.parse(raw) } : { ...DEFAULT_SETTINGS };
    } catch {
      return { ...DEFAULT_SETTINGS };
    }
  },
  setSettings: async (settings: Settings): Promise<void> => {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  },

  // ---- devices ------------------------------------------------------------
  listDevices: (): Promise<Device[]> => api.get<Device[]>("/devices"),
  saveDevice: async (device: Device): Promise<void> => {
    await api.put<void>("/devices", device);
  },
  deleteDevice: async (ip: string): Promise<boolean> => {
    await api.del<void>(`/devices/${encodeURIComponent(ip)}`);
    return true;
  },
  /** Relay camera credentials to the tenant's edge. They are NOT stored in the cloud —
   * control holds them in memory until the edge collects them, then discards. */
  setCredentials: async (ip: string, username: string, password: string): Promise<void> => {
    await api.post<void>(`/devices/${encodeURIComponent(ip)}/credentials`, { username, password });
  },
  renameDevice: async (ip: string, name: string): Promise<void> => {
    // No targeted rename endpoint — upsert the device with the new name.
    const list = await api.get<Device[]>("/devices");
    const device = list.find((d) => d.ip === ip);
    if (!device) throw new Error("Device not found.");
    await api.put<void>("/devices", { ...device, name });
  },
  /** Active ONVIF discovery is an edge capability, not exposed to the cloud — use
   * [`listDiscovered`] for cameras the edge has found and pushed up. */
  discoverDevices: (): Promise<never> => cloudOnly("Camera discovery"),
  /** Cameras the tenant's edge found on its LAN and pushed to the cloud. */
  listDiscovered: (): Promise<EdgeDiscovered[]> => api.get<EdgeDiscovered[]>("/devices/discovered"),
  /** IPs of cameras whose credentials have been set/relayed to the edge. */
  listConfigured: (): Promise<string[]> => api.get<string[]>("/devices/configured"),
  /** IPs the edge currently has a RESOLVED stream for (creds valid + camera reachable) —
   * the precise "online & correct" signal for the status dot. */
  listStreaming: (): Promise<string[]> => api.get<string[]>("/devices/streaming"),
  /** Ask the tenant's edge box(es) to re-run ONVIF discovery now (e.g. after a camera
   * changed IP). The edge reconciles moves by stable ONVIF id and pushes corrections back. */
  rescan: async (): Promise<void> => {
    await api.post<void>("/devices/rescan");
  },
  testCredentials: (): Promise<never> => cloudOnly("Credential test"),
  validateDevice: (): Promise<never> => cloudOnly("Credential validation"),

  // ---- live / snapshots (edge-owned, via the streaming gateway) ------------
  // Camera-frame capture (e.g. enroll/recognize from a live camera) has no cloud backend
  // yet — callers fall back to file upload. The `ip` arg is kept for call-site compat.
  captureSnapshot: (_ip?: string): Promise<never> => cloudOnly("Camera capture"),
  ensureStreams: (): Promise<never> => cloudOnly("Live streams"),

  // ---- alerts -------------------------------------------------------------
  listAlerts: (): Promise<Alert[]> => api.get<Alert[]>("/alerts"),
  alertStats: (): Promise<AlertStats> => api.get<AlertStats>("/alerts/stats"),
  createAlert: (level: string, message: string, room?: string | null, source?: string | null): Promise<Alert> =>
    api.post<Alert>("/alerts", { level, message, room, source }),
  deleteAlert: async (id: number): Promise<boolean> => {
    await api.del<void>(`/alerts/${id}`);
    return true;
  },
  deliverAlert: (id: number): Promise<string> => api.post<string>(`/alerts/${id}/deliver`),

  // ---- people -------------------------------------------------------------
  listPeople: (): Promise<Person[]> => api.get<Person[]>("/persons"),
  personPermissionKinds: (): Promise<string[]> => api.get<string[]>("/persons/permission-kinds"),
  savePerson: async (person: Person): Promise<void> => {
    await api.put<void>("/persons", person);
  },
  deletePerson: async (id: string): Promise<boolean> => {
    await api.del<void>(`/persons/${encodeURIComponent(id)}`);
    return true;
  },
  enrollFace: (personId: string, image: string, source: string, approved: boolean): Promise<string> =>
    api.post<string>(`/persons/${encodeURIComponent(personId)}/embeddings`, { image, source, approved }),
  listEmbeddings: (personId: string): Promise<PersonEmbedding[]> =>
    api.get<PersonEmbedding[]>(`/persons/${encodeURIComponent(personId)}/embeddings`),
  approveEmbedding: async (id: string): Promise<void> => {
    await api.post<void>(`/persons/embeddings/${encodeURIComponent(id)}/approve`);
  },
  deleteEmbedding: async (id: string): Promise<void> => {
    await api.del<void>(`/persons/embeddings/${encodeURIComponent(id)}`);
  },
  enrolledPersonIds: (): Promise<string[]> => api.get<string[]>("/persons/enrolled"),
  recognizeFace: (image: string, maxDistance?: number): Promise<RecognizeDetail> =>
    api.post<RecognizeDetail>("/persons/recognize", { image, max_distance: maxDistance }),

  // ---- beacons / telegram -------------------------------------------------
  listBeacons: (): Promise<NotificationTarget[]> => api.get<NotificationTarget[]>("/beacons"),
  getTelegramConfig: (): Promise<TelegramConfigState> => api.get<TelegramConfigState>("/telegram"),
  setPairingCode: async (pairingCode: string): Promise<void> => {
    await api.put<void>("/telegram", { pairing_code: pairingCode });
  },
  toggleBeacon: async (chatId: number, enabled: boolean): Promise<void> => {
    await api.post<void>(`/beacons/${chatId}/toggle`, { enabled });
  },
  renameBeacon: async (chatId: number, label: string): Promise<void> => {
    await api.patch<void>(`/beacons/${chatId}`, { label });
  },
  deleteBeacon: async (chatId: number): Promise<void> => {
    await api.del<void>(`/beacons/${chatId}`);
  },
  testBeacon: async (chatId: number): Promise<void> => {
    await api.post<void>(`/beacons/${chatId}/test`, {});
  },

  // ---- map ----------------------------------------------------------------
  // The whole floor plan is one document (rooms + walls + markers), replace-all.
  loadMap: (): Promise<MapDoc> => api.get<MapDoc>("/map"),
  saveMap: async (doc: MapDoc): Promise<void> => {
    await api.put<void>("/map", doc);
  },
  /** Recent presence events (baseline for the map's live pulse). */
  loadPresence: (limit = 100): Promise<Presence[]> =>
    api.get<Presence[]>(`/map/presence?limit=${limit}`),

  // ---- workflows ----------------------------------------------------------
  loadGraphs: <T = unknown>(): Promise<T[]> => api.get<T[]>("/workflows"),
  runStates: <T = unknown>(): Promise<T> => api.get<T>("/workflows/runs"),
  nodeCatalog: <T = unknown>(): Promise<T> => api.get<T>("/workflows/catalog"),
  createGraph: (name: string): Promise<number> => api.post<number>("/workflows", { name }),
  renameGraph: async (id: number, name: string): Promise<void> => {
    await api.patch<void>(`/workflows/${id}`, { name });
  },
  deleteGraph: async (id: number): Promise<void> => {
    await api.del<void>(`/workflows/${id}`);
  },
  setGraphTrigger: async (id: number, kind: string, everySecs?: number): Promise<void> => {
    await api.put<void>(`/workflows/${id}/trigger`, { kind, every_secs: everySecs });
  },
  setGraphConcurrency: async (id: number, kind: string, max?: number): Promise<void> => {
    await api.put<void>(`/workflows/${id}/concurrency`, { kind, max });
  },
  setGraphEnabled: async (id: number, enabled: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/enabled`, { value: enabled });
  },
  setGraphRunOrphans: async (id: number, runOrphans: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/run-orphans`, { value: runOrphans });
  },
  setGraphDebug: async (id: number, debug: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/debug`, { value: debug });
  },
  setGraphViewport: async (id: number, x: number, y: number, zoom: number): Promise<void> => {
    await api.put<void>(`/workflows/${id}/viewport`, { x, y, zoom });
  },
  addNode: (graphId: number, kind: string, x: number, y: number): Promise<number> =>
    api.post<number>(`/workflows/${graphId}/nodes`, { kind, x, y }),
  removeNode: async (graphId: number, nodeId: number): Promise<void> => {
    await api.del<void>(`/workflows/${graphId}/nodes/${nodeId}`);
  },
  moveNode: async (graphId: number, nodeId: number, x: number, y: number): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/move`, { x, y });
  },
  setNodeConfig: async (graphId: number, nodeId: number, config: unknown): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/config`, config);
  },
  setNodeMemoize: async (graphId: number, nodeId: number, memoize: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/memoize`, { memoize });
  },
  addEdge: async (graphId: number, from: number, fromPort: string, to: number, toPort: string): Promise<void> => {
    await api.post<void>(`/workflows/${graphId}/edges`, { from, from_port: fromPort, to, to_port: toPort });
  },
  removeEdge: async (graphId: number, idx: number): Promise<void> => {
    await api.del<void>(`/workflows/${graphId}/edges/${idx}`);
  },
  runGraph: (id: number): Promise<string> => api.post<string>(`/workflows/${id}/run`),

  // ---- workflow events + run history --------------------------------------
  listEvents: <T = unknown>(): Promise<T> => api.get<T>("/workflows/event-registry"),
  createEvent: async (
    name: string,
    category?: string,
    description?: string,
    fields?: { name: string; type: string }[],
  ): Promise<void> => {
    await api.post<void>("/workflows/event-registry", { name, category, description, fields });
  },
  deleteEvent: async (name: string): Promise<void> => {
    await api.del<void>(`/workflows/event-registry/${encodeURIComponent(name)}`);
  },
  listEventCategories: <T = unknown>(): Promise<T> => api.get<T>("/workflows/event-categories"),
  createEventCategory: async (name: string): Promise<void> => {
    await api.post<void>("/workflows/event-categories", { name });
  },
  runHistory: <T = unknown>(graphId?: number, limit?: number): Promise<T> => {
    const qs = new URLSearchParams();
    if (graphId != null) qs.set("graph", String(graphId));
    if (limit != null) qs.set("limit", String(limit));
    const tail = qs.toString();
    return api.get<T>(`/workflows/runs/history${tail ? `?${tail}` : ""}`);
  },
  runLogs: <T = unknown>(runId: string): Promise<T> =>
    api.get<T>(`/workflows/runs/${encodeURIComponent(runId)}/logs`),
  /** Out-of-band URL for a ShowImage node's latest debug frame (image/jpeg). The token
   * rides the query so it works as an `<img src>`; pass `bust` (e.g. Date.now()) to force
   * a reload when a new frame arrives. */
  runFrameUrl: (graph: number, node: number, bust?: number): string => {
    const qs = new URLSearchParams({ graph: String(graph), node: String(node) });
    if (auth.token) qs.set("token", auth.token);
    if (bust != null) qs.set("t", String(bust));
    return `${API_BASE}/workflows/runs/frame?${qs.toString()}`;
  },
};
