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
  type MapRoom,
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
  // EventSource auto-reconnects on error; keep it open.
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
  setTelegramConfig: async (botToken: string, pairingCode: string): Promise<string> => {
    await api.put<void>("/telegram", { bot_token: botToken, pairing_code: pairingCode });
    return "Saved.";
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
  testBeacon: (): Promise<never> => cloudOnly("Beacon test"),

  // ---- map ----------------------------------------------------------------
  loadMap: (): Promise<MapRoom[]> => api.get<MapRoom[]>("/map"),
  saveMap: async (rooms: MapRoom[]): Promise<void> => {
    await api.put<void>("/map", rooms);
  },

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
  setGraphTrigger: async (id: number, kind: string, everySecs?: number, minLevel?: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/trigger`, { kind, every_secs: everySecs, min_level: minLevel });
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
};
