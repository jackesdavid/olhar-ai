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
  type Face,
  type FaceCluster,
  type FaceDetail,
  type CanvasLayout,
  type Settings,
  type TelegramConfigState,
  type TuyaDevice,
} from "./types";

/** Unsubscribe handle returned by [`onEvent`]. */
export type UnlistenFn = () => void;

// ---- Dictionaries (#113): named, typed-column tables ----
/** A cell's type. Scalars (text/num/bool) + entity references (camera/recipient), extensible. */
export type CellType = "text" | "num" | "bool" | "camera" | "recipient";
/** One column of a dictionary's schema. `type` is serialized (Rust `#[serde(rename = "type")]`). */
export type DictColumn = { name: string; type: CellType };
/** One typed cell — tagged to survive schema round-trips: `{t, v}`. Camera = device id,
 *  Recipient = label (both string-backed, authored via a dropdown). */
export type DictCell =
  | { t: "text"; v: string }
  | { t: "num"; v: number }
  | { t: "bool"; v: boolean }
  | { t: "camera"; v: string }
  | { t: "recipient"; v: string };
/** A full dictionary (schema + rows), as returned by the API. */
export type Dictionary = {
  id: string;
  name: string;
  columns: DictColumn[];
  rows: DictCell[][];
  created_at?: string;
  updated_at?: string;
};
/** The create/update payload — name + schema + rows (rows conformed to columns server-side). */
export type DictionaryInput = { name: string; columns: DictColumn[]; rows: DictCell[][] };

// ---- Graph instances (#95): a live run context + its saved node values ----
/** One live instance of a graph (a run context with saved state / an armed latent window). */
export type WorkflowInstance = {
  instance_key: string;
  active: boolean;
  updated_at: string | null;
  run_id: string | null;
  status: string | null;
};
/** One node's last saved outputs within an instance (port name → display string). */
export type InstanceNodeValues = {
  node_id: number;
  label: string | null;
  values: Record<string, string>;
};
/** The full saved value context of one instance. */
export type InstanceValues = { instance_key: string; nodes: InstanceNodeValues[] };

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

/** What the edge found when it tried the credentials against the camera (card #88).
 *
 * These are deliberately distinct causes, not one "failed": a locked camera and a wrong
 * password need opposite advice from the user, and conflating them is what kept people
 * re-typing correct credentials at a camera that had stopped listening. */
export type ProbeStatus =
  /** Credentials accepted; the stream is describable. */
  | "ok"
  /** The camera evaluated the credentials and rejected them. */
  | "auth_failed"
  /** The camera is refusing every RTSP client regardless of credentials — needs a power cycle. */
  | "locked"
  /** Signed in, but there's no stream at that path. */
  | "not_found"
  /** No TCP connection: wrong IP, camera off, or a network boundary. */
  | "unreachable"
  /** Connected, but the camera never answered. */
  | "timeout"
  /** Anything we couldn't classify — `detail` carries the raw reason. */
  | "error";

export type ProbeResult = {
  status: ProbeStatus;
  /** Operator-facing detail — the camera's own reason line where we have one. */
  detail: string;
  /** Base64 JPEG proof, present only on `ok`. */
  snapshot: string | null;
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

  // ---- Tuya smart home ----------------------------------------------------
  /** Linked Tuya devices for the tenant (drives the node dropdowns + Integrations page). */
  listTuyaDevices: (): Promise<TuyaDevice[]> => api.get<TuyaDevice[]>("/tuya/devices"),
  /** Pull the project's linked devices from the Tuya Cloud and cache them. */
  syncTuyaDevices: (): Promise<{ synced: number }> => api.post<{ synced: number }>("/tuya/sync", {}),
  /** Whether the operator configured a Tuya project (env creds present). */
  getTuyaProvider: (): Promise<{ configured: boolean }> => api.get<{ configured: boolean }>("/tuya/provider"),

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
  /** Ask the edge to prove a camera's credentials, saving nothing (card #88).
   *
   *  This is the "test" half of test-then-save: call it first, and only call [`setCredentials`]
   *  on a verified `ok`. The edge speaks RTSP to the camera once and reports what actually
   *  happened, so the UI can tell "wrong password" apart from "camera locked" or "unreachable".
   *
   *  It replaced a save-then-poll loop that saved unproven creds (pointing go2rtc's reconnect
   *  loop at the camera) and retried every 2s for 25s. That burst trips the RTSP lockout on
   *  consumer cameras — a typo took a working camera offline until it was power-cycled — and
   *  every failure read as "wrong password", so users re-typed correct creds and made it worse.
   *  Hence: one attempt, no retries. Retrying is the bug, not the fix. */
  probeCredentials: (ip: string, username: string, password: string): Promise<ProbeResult> =>
    api.post<ProbeResult>(`/devices/${encodeURIComponent(ip)}/probe`, { username, password }),
  validateDevice: (): Promise<never> => cloudOnly("Credential validation"),

  // ---- live / snapshots (edge-owned, via the streaming gateway) ------------
  // Camera-frame capture (e.g. enroll/recognize from a live camera) has no cloud backend
  // yet — callers fall back to file upload. The `ip` arg is kept for call-site compat.
  captureSnapshot: (_ip?: string): Promise<never> => cloudOnly("Camera capture"),
  ensureStreams: (): Promise<never> => cloudOnly("Live streams"),
  /** A live JPEG snapshot of `ip` from the tenant's edge, via control's proxy (`GET
   * /devices/:ip/snapshot`) — for the workflow editor's Region-drawing UI (card: edge
   * tracking). Needs the Authorization header (image requests can't carry one), so this
   * fetches a Blob rather than returning a plain `<img src>` URL. */
  snapshotBlob: async (ip: string, timeoutMs?: number): Promise<Blob> => {
    const headers: Record<string, string> = {};
    if (auth.token) headers.Authorization = `Bearer ${auth.token}`;
    // A per-request timeout so a stuck edge/go2rtc can't hang the caller (the credential test polls
    // this in a loop and must keep moving).
    const signal = timeoutMs ? AbortSignal.timeout(timeoutMs) : undefined;
    const r = await fetch(`${API_BASE}/devices/${encodeURIComponent(ip)}/snapshot`, { headers, signal });
    if (!r.ok) throw new Error(`Snapshot failed (${r.status})`);
    return r.blob();
  },

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

  // ---- Faces view (#119): browse + cluster + bulk-label unknown faces ----
  listFaces: (): Promise<Face[]> => api.get<Face[]>("/faces"),
  listFaceClusters: (eps?: number): Promise<FaceCluster[]> =>
    api.get<FaceCluster[]>(`/faces/clusters${eps != null ? `?eps=${eps}` : ""}`),
  faceDetail: (id: string): Promise<FaceDetail> =>
    api.get<FaceDetail>(`/faces/${encodeURIComponent(id)}/detail`),
  /** On-demand facial landmarks (5 keypoints, in the crop's pixel coords) for the detail overlay. */
  faceLandmarks: (id: string): Promise<{ landmarks: [number, number][] }> =>
    api.get<{ landmarks: [number, number][] }>(`/faces/${encodeURIComponent(id)}/landmarks`),
  faceLayout: (opts: {
    eps?: number;
    scope?: "unknown" | "known" | "all";
    maxPerIsland?: number;
    maxIslands?: number;
    minSize?: number;
    spread?: number;
    since?: string;
    until?: string;
  } = {}): Promise<CanvasLayout> => {
    const q = new URLSearchParams();
    if (opts.eps != null) q.set("eps", String(opts.eps));
    if (opts.scope) q.set("scope", opts.scope);
    if (opts.maxPerIsland != null) q.set("max_per_island", String(opts.maxPerIsland));
    if (opts.maxIslands != null) q.set("max_islands", String(opts.maxIslands));
    if (opts.minSize != null) q.set("min_size", String(opts.minSize));
    if (opts.spread != null) q.set("spread", String(opts.spread));
    if (opts.since) q.set("since", opts.since);
    if (opts.until) q.set("until", opts.until);
    const s = q.toString();
    return api.get<CanvasLayout>(`/faces/layout${s ? `?${s}` : ""}`);
  },
  /** A displayable crop URL (token-in-query so it works as `<img src>`). `bust` forces a reload. */
  faceImageUrl: (id: string, opts?: { box?: boolean; bust?: number }): string => {
    const qs = new URLSearchParams();
    if (auth.token) qs.set("token", auth.token);
    if (opts?.box) qs.set("box", "1"); // full frame with the detection rectangle (context)
    if (opts?.bust) qs.set("t", String(opts.bust));
    return `${API_BASE}/faces/${encodeURIComponent(id)}/image?${qs.toString()}`;
  },
  /** Label a whole cluster → an existing person (personId) or a new one (newPersonName). */
  labelFaces: (
    faceIds: string[],
    target: { personId?: string; newPersonName?: string },
  ): Promise<{ person_id: string; labeled: number }> =>
    api.post<{ person_id: string; labeled: number }>("/faces/label", {
      face_ids: faceIds,
      person_id: target.personId,
      new_person_name: target.newPersonName,
    }),
  reassignFace: async (faceId: string, personId?: string): Promise<void> => {
    await api.post<void>("/faces/reassign", { face_id: faceId, person_id: personId ?? null });
  },
  ignoreFaces: async (faceIds: string[], ignored = true): Promise<void> => {
    await api.post<void>("/faces/ignore", { face_ids: faceIds, ignored });
  },
  /** HARD-delete faces (rows + MinIO crops) — for false positives / junk. Not recoverable. */
  deleteFaces: (faceIds: string[]): Promise<{ deleted: number }> =>
    api.post<{ deleted: number }>("/faces/delete", { face_ids: faceIds }),

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
  loadGraphs: <T = unknown>(opts?: { q?: string; group?: string }): Promise<T[]> => {
    const qs = new URLSearchParams();
    if (opts?.q) qs.set("q", opts.q);
    if (opts?.group) qs.set("group", opts.group);
    const tail = qs.toString();
    return api.get<T[]>(`/workflows${tail ? `?${tail}` : ""}`);
  },
  runStates: <T = unknown>(): Promise<T> => api.get<T>("/workflows/runs"),
  nodeCatalog: <T = unknown>(): Promise<T> => api.get<T>("/workflows/catalog"),
  createGraph: (name: string): Promise<string> => api.post<string>("/workflows", { name }),
  renameGraph: async (id: string, name: string): Promise<void> => {
    await api.patch<void>(`/workflows/${id}`, { name });
  },
  setGraphDescription: async (id: string, value: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/description`, { value });
  },
  setGraphGroup: async (id: string, value: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/group`, { value });
  },
  duplicateGraph: (id: string): Promise<string> => api.post<string>(`/workflows/${id}/duplicate`),

  /** Templates a tenant can start from: its own + every global one (#102). */
  listTemplates: <T = unknown>(): Promise<T[]> => api.get<T[]>("/workflows/templates"),
  /** Clone a template into the caller's tenant as a normal, disabled workflow; returns its id. */
  useTemplate: (id: string): Promise<string> => api.post<string>(`/workflows/templates/${id}/use`),
  /** Mark/unmark a graph as one of the tenant's own templates. */
  setGraphTemplate: async (id: string, value: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/template`, { value });
  },
  /** Publish/unpublish a template to the GLOBAL catalog — master only (403 otherwise). */
  setGraphGlobal: async (id: string, value: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/global`, { value });
  },
  // ---- Dictionaries (#113): named typed-column tables, reusable across workflows ----
  /** All of the tenant's dictionaries (schema + rows), newest first. */
  listDictionaries: (): Promise<Dictionary[]> => api.get<Dictionary[]>("/dictionaries"),
  /** One dictionary by id. */
  getDictionary: (id: string): Promise<Dictionary> => api.get<Dictionary>(`/dictionaries/${id}`),
  /** Create a dictionary from a name + schema + rows; returns its new id. */
  createDictionary: (body: DictionaryInput): Promise<string> =>
    api.post<string>("/dictionaries", body),
  /** Replace a dictionary's full content (name, columns, rows). Returns the saved dictionary. */
  updateDictionary: (id: string, body: DictionaryInput): Promise<Dictionary> =>
    api.put<Dictionary>(`/dictionaries/${id}`, body),
  /** Delete a dictionary. 409 (with the referencing workflow names) while a variable still uses it. */
  deleteDictionary: async (id: string): Promise<void> => {
    await api.del<void>(`/dictionaries/${id}`);
  },

  // ---- Graph instances (#95): inspect a graph's live run contexts + their saved values ----
  /** Live instances of a graph (distinct run contexts with saved state / active windows). */
  listInstances: (id: string): Promise<WorkflowInstance[]> =>
    api.get<WorkflowInstance[]>(`/workflows/${id}/instances`),
  /** The saved per-node value context of one instance (key may be "" for Single-scope graphs). */
  getInstanceValues: (id: string, key: string): Promise<InstanceValues> =>
    api.get<InstanceValues>(`/workflows/${id}/instance-values?key=${encodeURIComponent(key)}`),

  deleteGraph: async (id: string): Promise<void> => {
    await api.del<void>(`/workflows/${id}`);
  },
  setGraphTrigger: async (id: string, kind: string, everySecs?: number): Promise<void> => {
    await api.put<void>(`/workflows/${id}/trigger`, { kind, every_secs: everySecs });
  },
  setGraphConcurrency: async (id: string, kind: string, max?: number): Promise<void> => {
    await api.put<void>(`/workflows/${id}/concurrency`, { kind, max });
  },
  /** What a graph runs one of (#95): "single" (default) or "per_camera". */
  setGraphInstanceScope: async (id: string, kind: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/instance-scope`, { kind });
  },
  setGraphEnabled: async (id: string, enabled: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/enabled`, { value: enabled });
  },
  setGraphRunOrphans: async (id: string, runOrphans: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/run-orphans`, { value: runOrphans });
  },
  setGraphDebug: async (id: string, debug: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/debug`, { value: debug });
  },
  setGraphViewport: async (id: string, x: number, y: number, zoom: number): Promise<void> => {
    await api.put<void>(`/workflows/${id}/viewport`, { x, y, zoom });
  },
  setEdgeVias: async (id: string, vias: Record<string, [number, number][]>): Promise<void> => {
    await api.put<void>(`/workflows/${id}/edge-vias`, vias);
  },
  setGraphVariables: async (
    id: string,
    variables: {
      name: string;
      type: string;
      default?: string | null;
      param?: boolean;
      description?: string | null;
    }[],
  ): Promise<void> => {
    await api.put<void>(`/workflows/${id}/variables`, variables);
  },
  /** Explicit Save (#122): write the whole current graph's canvas (nodes/edges/positions/configs/
   *  viewport/wire-bends) in one call. Metadata (name/trigger/enabled/…) is preserved server-side. */
  saveGraph: async (
    graphId: string,
    body: { nodes: unknown[]; edges: unknown[]; viewport?: unknown; edge_vias?: unknown; next_id?: number },
  ): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/graph`, body);
  },
  addNode: (graphId: string, kind: string, x: number, y: number): Promise<number> =>
    api.post<number>(`/workflows/${graphId}/nodes`, { kind, x, y }),
  removeNode: async (graphId: string, nodeId: number): Promise<void> => {
    await api.del<void>(`/workflows/${graphId}/nodes/${nodeId}`);
  },
  moveNode: async (graphId: string, nodeId: number, x: number, y: number): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/move`, { x, y });
  },
  setNodeConfig: async (graphId: string, nodeId: number, config: unknown): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/config`, config);
  },
  setNodeMemoize: async (graphId: string, nodeId: number, memoize: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/memoize`, { memoize });
  },
  addEdge: async (graphId: string, from: number, fromPort: string, to: number, toPort: string): Promise<void> => {
    await api.post<void>(`/workflows/${graphId}/edges`, { from, from_port: fromPort, to, to_port: toPort });
  },
  removeEdge: async (graphId: string, idx: number): Promise<void> => {
    await api.del<void>(`/workflows/${graphId}/edges/${idx}`);
  },
  /** Manually run a graph. `inputs` (optional) supplies per-launch values for the graph's input
   *  variables (#102), by variable name — they override the stored defaults for this run only. */
  runGraph: (id: string, inputs?: Record<string, string>): Promise<string> =>
    api.post<string>(`/workflows/${id}/run`, inputs && Object.keys(inputs).length ? { inputs } : undefined),

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
  runHistory: <T = unknown>(graphId?: string, limit?: number): Promise<T> => {
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
  runFrameUrl: (graph: string, node: number, bust?: number): string => {
    const qs = new URLSearchParams({ graph: String(graph), node: String(node) });
    if (auth.token) qs.set("token", auth.token);
    if (bust != null) qs.set("t", String(bust));
    return `${API_BASE}/workflows/runs/frame?${qs.toString()}`;
  },
  nodeStateUrl: (graph: string, node: number): string => {
    const qs = new URLSearchParams({ graph: String(graph), node: String(node) });
    if (auth.token) qs.set("token", auth.token);
    return `${API_BASE}/workflows/runs/node-state?${qs.toString()}`;
  },
};
