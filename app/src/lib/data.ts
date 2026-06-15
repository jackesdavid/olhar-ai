// Unified data layer: one API the pages call, dispatching to local Tauri `invoke`
// (desktop) or the control-plane HTTP API (browser/cloud) per command. This is what
// lets the desktop app and the cloud client share one codebase — pages never import
// `invoke` directly; they call `data.*` (and `onEvent` for live updates).
//
// Commands that only exist on the edge/desktop (ONVIF discovery, snapshots, the live
// stream, chat, voice, beacon test) throw a clear error in cloud mode — pages guard
// those actions with `isTauri()`.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { api } from "./api";
import { isTauri } from "./auth.svelte";
import {
  DEFAULT_SETTINGS,
  type Alert,
  type AlertStats,
  type Device,
  type DiscoveredDevice,
  type MapRoom,
  type NotificationTarget,
  type Person,
  type PersonEmbedding,
  type RecognizeDetail,
  type Settings,
  type StreamInfo,
  type TelegramConfigState,
} from "./types";

export { isTauri };

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

const TAURI = isTauri();
const SETTINGS_KEY = "sentinel.settings";

function cloudOnly(feature: string): never {
  throw new Error(`${feature} is only available in the desktop app.`);
}

/** Subscribe to a backend push event. In cloud mode there is no push channel yet, so
 * this is a no-op (pages re-fetch after mutations). Returns an unlisten function. */
export async function onEvent(name: string, cb: (payload?: unknown) => void): Promise<UnlistenFn> {
  if (!TAURI) return () => {};
  return listen(name, (e) => cb(e.payload));
}

export const data = {
  // ---- settings (UI-local: Tauri file, else localStorage) -----------------
  getSettings: async (): Promise<Settings> => {
    if (TAURI) return invoke<Settings>("get_settings");
    try {
      const raw = localStorage.getItem(SETTINGS_KEY);
      return raw ? { ...DEFAULT_SETTINGS, ...JSON.parse(raw) } : { ...DEFAULT_SETTINGS };
    } catch {
      return { ...DEFAULT_SETTINGS };
    }
  },
  setSettings: async (settings: Settings): Promise<void> => {
    if (TAURI) return invoke("set_settings", { settings });
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  },

  // ---- devices ------------------------------------------------------------
  listDevices: (): Promise<Device[]> =>
    TAURI ? invoke<Device[]>("list_devices") : api.get<Device[]>("/devices"),
  saveDevice: async (device: Device): Promise<void> => {
    if (TAURI) return invoke("save_device", { device });
    await api.put<void>("/devices", device);
  },
  deleteDevice: async (ip: string): Promise<boolean> => {
    if (TAURI) return invoke<boolean>("delete_device", { ip });
    await api.del<void>(`/devices/${encodeURIComponent(ip)}`);
    return true;
  },
  renameDevice: async (ip: string, name: string): Promise<void> => {
    if (TAURI) return invoke("rename_device", { ip, name });
    // Cloud has no targeted rename — upsert the device with the new name.
    const list = await api.get<Device[]>("/devices");
    const device = list.find((d) => d.ip === ip);
    if (!device) throw new Error("Device not found.");
    await api.put<void>("/devices", { ...device, name });
  },
  discoverDevices: (): Promise<DiscoveredDevice[]> =>
    TAURI ? invoke<DiscoveredDevice[]>("discover_devices") : cloudOnly("Camera discovery"),
  /** Cameras the tenant's edge found on its LAN and pushed to the cloud (cloud mode). */
  listDiscovered: (): Promise<EdgeDiscovered[]> =>
    TAURI ? Promise.resolve([]) : api.get<EdgeDiscovered[]>("/devices/discovered"),
  testCredentials: (xaddrOrIp: string, username: string, password: string): Promise<string> =>
    TAURI ? invoke<string>("test_credentials", { xaddrOrIp, username, password }) : cloudOnly("Credential test"),
  validateDevice: (ip: string, username: string, password: string): Promise<Device> =>
    TAURI ? invoke<Device>("validate_device", { ip, username, password }) : cloudOnly("Credential validation"),

  // ---- live / snapshots (edge-owned) --------------------------------------
  captureSnapshot: (ip: string): Promise<string> =>
    TAURI ? invoke<string>("capture_snapshot", { ip }) : cloudOnly("Snapshots"),
  ensureStreams: (): Promise<StreamInfo[]> =>
    TAURI ? invoke<StreamInfo[]>("ensure_streams") : cloudOnly("Live streams"),

  // ---- alerts -------------------------------------------------------------
  listAlerts: (): Promise<Alert[]> =>
    TAURI ? invoke<Alert[]>("list_alerts") : api.get<Alert[]>("/alerts"),
  alertStats: (): Promise<AlertStats> =>
    TAURI ? invoke<AlertStats>("alert_stats") : api.get<AlertStats>("/alerts/stats"),
  createAlert: (level: string, message: string, room?: string | null, source?: string | null): Promise<Alert> =>
    TAURI
      ? invoke<Alert>("create_alert", { level, message, room, source })
      : api.post<Alert>("/alerts", { level, message, room, source }),
  deleteAlert: async (id: number): Promise<boolean> => {
    if (TAURI) return invoke<boolean>("delete_alert", { id });
    await api.del<void>(`/alerts/${id}`);
    return true;
  },
  deliverAlert: (id: number): Promise<string> =>
    TAURI ? invoke<string>("deliver_alert", { id }) : api.post<string>(`/alerts/${id}/deliver`),

  // ---- people -------------------------------------------------------------
  listPeople: (): Promise<Person[]> =>
    TAURI ? invoke<Person[]>("list_people") : api.get<Person[]>("/persons"),
  personPermissionKinds: (): Promise<string[]> =>
    TAURI ? invoke<string[]>("person_permission_kinds") : api.get<string[]>("/persons/permission-kinds"),
  savePerson: async (person: Person): Promise<void> => {
    if (TAURI) return invoke("save_person", { person });
    await api.put<void>("/persons", person);
  },
  deletePerson: async (id: string): Promise<boolean> => {
    if (TAURI) return invoke<boolean>("delete_person", { id });
    await api.del<void>(`/persons/${encodeURIComponent(id)}`);
    return true;
  },
  enrollFace: (personId: string, image: string, source: string, approved: boolean): Promise<string> =>
    TAURI
      ? invoke<string>("enroll_face", { personId, image, source, approved })
      : api.post<string>(`/persons/${encodeURIComponent(personId)}/embeddings`, { image, source, approved }),
  listEmbeddings: (personId: string): Promise<PersonEmbedding[]> =>
    TAURI
      ? invoke<PersonEmbedding[]>("list_embeddings", { personId })
      : api.get<PersonEmbedding[]>(`/persons/${encodeURIComponent(personId)}/embeddings`),
  approveEmbedding: async (id: string): Promise<void> => {
    if (TAURI) return invoke("approve_embedding", { id });
    await api.post<void>(`/persons/embeddings/${encodeURIComponent(id)}/approve`);
  },
  deleteEmbedding: async (id: string): Promise<void> => {
    if (TAURI) return invoke("delete_embedding", { id });
    await api.del<void>(`/persons/embeddings/${encodeURIComponent(id)}`);
  },
  enrolledPersonIds: (): Promise<string[]> =>
    TAURI ? invoke<string[]>("enrolled_person_ids") : api.get<string[]>("/persons/enrolled"),
  recognizeFace: (image: string, maxDistance?: number): Promise<RecognizeDetail> =>
    TAURI
      ? invoke<RecognizeDetail>("recognize_face", { image, maxDistance })
      : api.post<RecognizeDetail>("/persons/recognize", { image, max_distance: maxDistance }),

  // ---- beacons / telegram -------------------------------------------------
  listBeacons: (): Promise<NotificationTarget[]> =>
    TAURI ? invoke<NotificationTarget[]>("list_beacons") : api.get<NotificationTarget[]>("/beacons"),
  getTelegramConfig: (): Promise<TelegramConfigState> =>
    TAURI ? invoke<TelegramConfigState>("get_telegram_config") : api.get<TelegramConfigState>("/telegram"),
  setTelegramConfig: async (botToken: string, pairingCode: string): Promise<string> => {
    if (TAURI) return invoke<string>("set_telegram_config", { botToken, pairingCode });
    await api.put<void>("/telegram", { bot_token: botToken, pairing_code: pairingCode });
    return "Saved.";
  },
  toggleBeacon: async (chatId: number, enabled: boolean): Promise<void> => {
    if (TAURI) return invoke("toggle_beacon", { chatId, enabled });
    await api.post<void>(`/beacons/${chatId}/toggle`, { enabled });
  },
  renameBeacon: async (chatId: number, label: string): Promise<void> => {
    if (TAURI) return invoke("rename_beacon", { chatId, label });
    await api.patch<void>(`/beacons/${chatId}`, { label });
  },
  deleteBeacon: async (chatId: number): Promise<void> => {
    if (TAURI) return invoke("delete_beacon", { chatId });
    await api.del<void>(`/beacons/${chatId}`);
  },
  testBeacon: (label: string): Promise<string> =>
    TAURI ? invoke<string>("test_beacon", { label }) : cloudOnly("Beacon test"),

  // ---- map ----------------------------------------------------------------
  loadMap: (): Promise<MapRoom[]> =>
    TAURI ? invoke<MapRoom[]>("load_map") : api.get<MapRoom[]>("/map"),
  saveMap: async (rooms: MapRoom[]): Promise<void> => {
    if (TAURI) return invoke("save_map", { rooms });
    await api.put<void>("/map", rooms);
  },

  // ---- workflows ----------------------------------------------------------
  loadGraphs: <T = unknown>(): Promise<T[]> =>
    TAURI ? invoke<T[]>("load_graphs") : api.get<T[]>("/workflows"),
  runStates: <T = unknown>(): Promise<T> =>
    TAURI ? invoke<T>("run_states") : api.get<T>("/workflows/runs"),
  nodeCatalog: <T = unknown>(): Promise<T> =>
    TAURI ? invoke<T>("node_catalog") : api.get<T>("/workflows/catalog"),
  createGraph: (name: string): Promise<number> =>
    TAURI ? invoke<number>("create_graph", { name }) : api.post<number>("/workflows", { name }),
  renameGraph: async (id: number, name: string): Promise<void> => {
    if (TAURI) return invoke("rename_graph", { id, name });
    await api.patch<void>(`/workflows/${id}`, { name });
  },
  deleteGraph: async (id: number): Promise<void> => {
    if (TAURI) return invoke("delete_graph", { id });
    await api.del<void>(`/workflows/${id}`);
  },
  setGraphTrigger: async (id: number, kind: string, everySecs?: number, minLevel?: string): Promise<void> => {
    if (TAURI) return invoke("set_graph_trigger", { id, kind, everySecs, minLevel });
    await api.put<void>(`/workflows/${id}/trigger`, { kind, every_secs: everySecs, min_level: minLevel });
  },
  setGraphConcurrency: async (id: number, kind: string, max?: number): Promise<void> => {
    if (TAURI) return invoke("set_graph_concurrency", { id, kind, max });
    await api.put<void>(`/workflows/${id}/concurrency`, { kind, max });
  },
  setGraphEnabled: async (id: number, enabled: boolean): Promise<void> => {
    if (TAURI) return invoke("set_graph_enabled", { id, enabled });
    await api.put<void>(`/workflows/${id}/enabled`, { value: enabled });
  },
  setGraphRunOrphans: async (id: number, runOrphans: boolean): Promise<void> => {
    if (TAURI) return invoke("set_graph_run_orphans", { id, runOrphans });
    await api.put<void>(`/workflows/${id}/run-orphans`, { value: runOrphans });
  },
  setGraphDebug: async (id: number, debug: boolean): Promise<void> => {
    if (TAURI) return invoke("set_graph_debug", { id, debug });
    await api.put<void>(`/workflows/${id}/debug`, { value: debug });
  },
  setGraphViewport: async (id: number, x: number, y: number, zoom: number): Promise<void> => {
    if (TAURI) return invoke("set_graph_viewport", { id, x, y, zoom });
    await api.put<void>(`/workflows/${id}/viewport`, { x, y, zoom });
  },
  addNode: (graphId: number, kind: string, x: number, y: number): Promise<number> =>
    TAURI
      ? invoke<number>("add_node", { graphId, kind, x, y })
      : api.post<number>(`/workflows/${graphId}/nodes`, { kind, x, y }),
  removeNode: async (graphId: number, nodeId: number): Promise<void> => {
    if (TAURI) return invoke("remove_node", { graphId, nodeId });
    await api.del<void>(`/workflows/${graphId}/nodes/${nodeId}`);
  },
  moveNode: async (graphId: number, nodeId: number, x: number, y: number): Promise<void> => {
    if (TAURI) return invoke("move_node", { graphId, nodeId, x, y });
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/move`, { x, y });
  },
  setNodeConfig: async (graphId: number, nodeId: number, config: unknown): Promise<void> => {
    if (TAURI) return invoke("set_node_config", { graphId, nodeId, config });
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/config`, config);
  },
  addEdge: async (graphId: number, from: number, fromPort: string, to: number, toPort: string): Promise<void> => {
    if (TAURI) return invoke("add_edge", { graphId, from, fromPort, to, toPort });
    await api.post<void>(`/workflows/${graphId}/edges`, { from, from_port: fromPort, to, to_port: toPort });
  },
  removeEdge: async (graphId: number, idx: number): Promise<void> => {
    if (TAURI) return invoke("remove_edge", { graphId, idx });
    await api.del<void>(`/workflows/${graphId}/edges/${idx}`);
  },
  runGraph: (id: number): Promise<string> =>
    TAURI ? invoke<string>("run_graph", { id }) : api.post<string>(`/workflows/${id}/run`),
};
