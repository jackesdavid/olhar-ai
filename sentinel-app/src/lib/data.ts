// Data layer: one API the pages call, dispatching to the control-plane HTTP API. Sentinel
// is web-only now (the Tauri desktop shell was removed), so every command goes to the cloud
// backend (`api.*`) or localStorage for UI-local state. A few capabilities have no cloud
// backend yet and throw a clear error via `cloudOnly(...)`; the pages must not surface those
// affordances. Snapshots are NOT among them any more — camera capture goes through the edge
// proxy (see `snapshotBlob`). A stub whose comment says callers fall back is only true if a
// caller actually falls back; check before adding one.
import { api } from "./api";
import { API_BASE } from "./config";
import { auth, mediaToken, mediaTokenExpiresAt } from "./auth.svelte";
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
  type TuyaProvider,
  type CanvasLayout,
  type Settings,
  type TelegramConfigState,
  type TuyaDevice,
  type TuyaDiagnosis,
  type TuyaTestResult,
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
export type DictionaryInput = {
  name: string;
  columns: DictColumn[];
  rows: DictCell[][];
};

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
export type InstanceValues = {
  instance_key: string;
  nodes: InstanceNodeValues[];
};

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

/** A camera the edge has a resolved stream for, plus the codec it probed off the camera's own
 * SDP ("H264"/"H265") — `null` on an edge build that hasn't reported it yet. */
export type StreamingCamera = {
  ip: string;
  codec: string | null;
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

/** A live JPEG frame of `ip` from the tenant's edge, via control's proxy
 * (`GET /devices/:ip/snapshot`).
 *
 * A module-level function rather than an object method because two exported entries need it —
 * `snapshotBlob` (the workflow editor's region-drawing UI) and `captureSnapshot` (enroll /
 * recognize) — and an object literal cannot reference its own siblings.
 *
 * Fetches a Blob rather than handing back an `<img src>` URL: the request needs the
 * Authorization header, and an image element cannot carry one. */
async function snapshotBlob(ip: string, timeoutMs?: number): Promise<Blob> {
  const headers: Record<string, string> = {};
  if (auth.token) headers.Authorization = `Bearer ${auth.token}`;
  // A timeout is optional because some callers poll this in a loop and must keep moving.
  const signal = timeoutMs ? AbortSignal.timeout(timeoutMs) : undefined;
  const r = await fetch(`${API_BASE}/devices/${encodeURIComponent(ip)}/snapshot`, {
    headers,
    signal,
  });
  if (!r.ok) throw new Error(`Snapshot failed (${r.status})`);
  return r.blob();
}

function cloudOnly(feature: string): never {
  throw new Error(`${feature} is not available in the web app yet.`);
}

// ---- live push: one shared SSE stream from the control relay ------------------
// The browser opens a SINGLE `EventSource` to control's `/workflows/events` (which relays
// sentinel-flow's run progress + alert/device pokes, tenant-scoped) and demultiplexes it by
// the event's `event` field to the per-name subscribers below.
const cloudSubs = new Map<string, Set<(payload?: unknown) => void>>();
let cloudStream: EventSource | null = null;

/** Pending deliberate reconnect, scheduled just before the media token expires. */
let cloudRenew: ReturnType<typeof setTimeout> | null = null;

/**
 * Reopen the stream shortly BEFORE its token expires.
 *
 * A native `EventSource` retries the exact URL it was constructed with, so once the token in that
 * URL expires every automatic retry 401s — the browser gives up, and the stream only comes back via
 * the `CLOSED` path below, after a visible gap. Renewing early turns that into one deliberate
 * reopen at a moment of our choosing; `stream-open` subscribers already re-seed the delta-only
 * state, which is exactly what that hook exists for.
 */
function scheduleCloudRenew(): void {
  if (cloudRenew) clearTimeout(cloudRenew);
  cloudRenew = null;
  const exp = mediaTokenExpiresAt();
  if (exp == null) return; // session-token fallback: 24h, nothing to renew around
  // Five minutes of headroom, and never a hot loop if the clock is odd or expiry is close.
  const delay = Math.max(30_000, exp - Date.now() - 5 * 60_000);
  cloudRenew = setTimeout(() => {
    if (!cloudStream) return;
    cloudStream.close();
    cloudStream = null;
    ensureCloudStream();
  }, delay);
}

function ensureCloudStream(): void {
  if (cloudStream || typeof EventSource === "undefined" || !auth.token) return;
  // A media token, not the session JWT: this URL is a query string, so it reaches proxy access
  // logs (see the Traefik RequestPath note) and cannot carry a header. Falls back to the session
  // token until the first mint lands, which control still accepts during the §1.8 overlap.
  const tok = mediaToken();
  if (!tok) return;
  const url = `${API_BASE}/workflows/events?token=${encodeURIComponent(tok)}`;
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
  scheduleCloudRenew();
}

function closeCloudStreamIfIdle(): void {
  if (cloudStream && [...cloudSubs.values()].every((s) => s.size === 0)) {
    cloudStream.close();
    cloudStream = null;
    // Drop the pending renewal too, or it fires against a stream nobody is listening to.
    if (cloudRenew) clearTimeout(cloudRenew);
    cloudRenew = null;
  }
}

/** Subscribe to a backend push event over the relayed SSE stream (control → sentinel-flow).
 * Returns an unsubscribe function. */
export async function onEvent(
  name: string,
  cb: (payload?: unknown) => void,
): Promise<UnlistenFn> {
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
      return raw
        ? { ...DEFAULT_SETTINGS, ...JSON.parse(raw) }
        : { ...DEFAULT_SETTINGS };
    } catch {
      return { ...DEFAULT_SETTINGS };
    }
  },
  setSettings: async (settings: Settings): Promise<void> => {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  },

  // ---- Tuya smart home ----------------------------------------------------
  /** Linked Tuya devices for the tenant (drives the node dropdowns + Integrations page). */
  listTuyaDevices: (): Promise<TuyaDevice[]> =>
    api.get<TuyaDevice[]>("/tuya/devices"),
  /** Pull the project's linked devices from the Tuya Cloud and cache them. */
  syncTuyaDevices: (): Promise<{ synced: number; unreadable: string[] }> =>
    api.post<{ synced: number; unreadable: string[] }>("/tuya/sync", {}),
  /** Adding a SECOND Smart Life account is a console action — Tuya publishes no API for the QR —
   *  so `console_url` is how we help: a deep link to the tab where it happens. `console_url_exact`
   *  is false when we only know the region and the link lands on the project LIST instead, which
   *  is when it is worth asking for a pasted URL. */
  setTuyaConsoleUrl: (url: string): Promise<void> =>
    api.put<void>("/tuya/console", { url }),
  /** Whether Tuya is connected for this tenant, via which source, and where to link another
   *  Smart Life account. */
  getTuyaProvider: (): Promise<TuyaProvider> => api.get<TuyaProvider>("/tuya/provider"),
  /** Save the tenant's own Tuya project credentials (validated server-side before saving). */
  setTuyaConfig: async (
    access_id: string,
    access_key: string,
    endpoint: string,
  ): Promise<void> => {
    await api.put<void>("/tuya/config", { access_id, access_key, endpoint });
  },
  /** Disconnect the tenant's Tuya account (remove saved credentials). */
  deleteTuyaConfig: async (): Promise<void> => {
    await api.del<void>("/tuya/config");
  },
  /** Why the connection is not working, as a four-step checklist (#182). Pass credentials to
   *  probe them BEFORE saving; omit them to re-check what is already saved. Answers 200 with a
   *  diagnosis even when everything failed — "wrong data centre" is an answer, not an error. */
  diagnoseTuya: (creds?: {
    access_id: string;
    access_key: string;
    endpoint: string;
  }): Promise<TuyaDiagnosis> => api.post<TuyaDiagnosis>("/tuya/diagnose", creds ?? {}),
  /** Prove a device answers. No `code` = read its status and change nothing. */
  testTuyaDevice: (
    deviceId: string,
    fn?: { code: string; value: unknown },
  ): Promise<TuyaTestResult> =>
    api.post<TuyaTestResult>(
      `/tuya/devices/${encodeURIComponent(deviceId)}/test`,
      fn ? { code: fn.code, value: fn.value } : {},
    ),
  /** Set a device's friendly per-function labels (`{ code: label }`). Survives sync. */
  setTuyaFunctionLabels: async (
    deviceId: string,
    labels: Record<string, string>,
  ): Promise<void> => {
    await api.patch<void>(
      `/tuya/devices/${encodeURIComponent(deviceId)}/labels`,
      { labels },
    );
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
   * control holds them in memory until the edge collects them, then discards.
   *
   * `rtspUrl` is for a source that does not speak ONVIF (#175) — a phone running an RTSP app, or a
   * camera whose ONVIF is broken. It travels the same memory-only channel as the password, because
   * an RTSP URL routinely embeds credentials. Omit it for discovered cameras: the edge resolves
   * their URL itself, and sending one would override that. */
  setCredentials: async (
    ip: string,
    username: string,
    password: string,
    rtspUrl?: string,
  ): Promise<void> => {
    await api.post<void>(`/devices/${encodeURIComponent(ip)}/credentials`, {
      username,
      password,
      ...(rtspUrl ? { rtsp_url: rtspUrl } : {}),
    });
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
  listDiscovered: (): Promise<EdgeDiscovered[]> =>
    api.get<EdgeDiscovered[]>("/devices/discovered"),
  /** IPs of cameras whose credentials have been set/relayed to the edge. */
  listConfigured: (): Promise<string[]> =>
    api.get<string[]>("/devices/configured"),
  /** Cameras the edge currently has a RESOLVED stream for (creds valid + camera reachable) —
   * the precise "online & correct" signal for the status dot, plus the codec the edge probed
   * off each camera's own SDP ("H264"/"H265"), when known. */
  listStreaming: (): Promise<StreamingCamera[]> =>
    api.get<StreamingCamera[]>("/devices/streaming"),
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
   *  loop at the camera) and retried every 2s for 25s. That burst trips the RTSP lockout on
   *  consumer cameras — a typo took a working camera offline until it was power-cycled — and
   *  every failure read as "wrong password", so users re-typed correct creds and made it worse.
   *  Hence: one attempt, no retries. Retrying is the bug, not the fix. */
  probeCredentials: (
    ip: string,
    username: string,
    password: string,
  ): Promise<ProbeResult> =>
    api.post<ProbeResult>(`/devices/${encodeURIComponent(ip)}/probe`, {
      username,
      password,
    }),
  validateDevice: (): Promise<never> => cloudOnly("Credential validation"),

  // ---- live / snapshots (edge-owned, via the streaming gateway) ------------
  /** A camera frame as a `data:` URL, for enroll / recognize.
   *
   * This used to be a `cloudOnly()` stub whose comment claimed callers fell back to file upload.
   * They did not: both `captureFromCamera` handlers awaited it and showed the thrown "not
   * available in the web app yet" as an error, so the button never worked. Meanwhile the real
   * path was two functions below — `snapshotBlob` has been fetching live frames through control's
   * edge proxy for the workflow editor's region-drawing UI the whole time.
   *
   * The callers want a data URL (they hand it straight to enroll/recognize), so this is
   * `snapshotBlob` plus a Blob→data-URL read. */
  captureSnapshot: async (ip: string, timeoutMs?: number): Promise<string> => {
    if (!ip) throw new Error("Pick a camera first.");
    const blob = await snapshotBlob(ip, timeoutMs);
    return await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(new Error("Could not read the camera frame."));
      reader.onload = () => resolve(String(reader.result));
      reader.readAsDataURL(blob);
    });
  },
  ensureStreams: (): Promise<never> => cloudOnly("Live streams"),
  snapshotBlob,

  // ---- alerts -------------------------------------------------------------
  listAlerts: (): Promise<Alert[]> => api.get<Alert[]>("/alerts"),
  alertStats: (): Promise<AlertStats> => api.get<AlertStats>("/alerts/stats"),
  createAlert: (
    level: string,
    message: string,
    room?: string | null,
    source?: string | null,
  ): Promise<Alert> =>
    api.post<Alert>("/alerts", { level, message, room, source }),
  deleteAlert: async (id: number): Promise<boolean> => {
    await api.del<void>(`/alerts/${id}`);
    return true;
  },
  deliverAlert: (id: number): Promise<string> =>
    api.post<string>(`/alerts/${id}/deliver`),

  // ---- people -------------------------------------------------------------
  listPeople: (): Promise<Person[]> => api.get<Person[]>("/persons"),
  personPermissionKinds: (): Promise<string[]> =>
    api.get<string[]>("/persons/permission-kinds"),
  savePerson: async (person: Person): Promise<void> => {
    await api.put<void>("/persons", person);
  },
  deletePerson: async (id: string): Promise<boolean> => {
    await api.del<void>(`/persons/${encodeURIComponent(id)}`);
    return true;
  },
  enrollFace: (
    personId: string,
    image: string,
    source: string,
    approved: boolean,
  ): Promise<string> =>
    api.post<string>(`/persons/${encodeURIComponent(personId)}/embeddings`, {
      image,
      source,
      approved,
    }),
  listEmbeddings: (personId: string): Promise<PersonEmbedding[]> =>
    api.get<PersonEmbedding[]>(
      `/persons/${encodeURIComponent(personId)}/embeddings`,
    ),
  approveEmbedding: async (id: string): Promise<void> => {
    await api.post<void>(
      `/persons/embeddings/${encodeURIComponent(id)}/approve`,
    );
  },
  deleteEmbedding: async (id: string): Promise<void> => {
    await api.del<void>(`/persons/embeddings/${encodeURIComponent(id)}`);
  },
  enrolledPersonIds: (): Promise<string[]> =>
    api.get<string[]>("/persons/enrolled"),
  recognizeFace: (
    image: string,
    maxDistance?: number,
  ): Promise<RecognizeDetail> =>
    api.post<RecognizeDetail>("/persons/recognize", {
      image,
      max_distance: maxDistance,
    }),

  // ---- Faces view (#119): browse + cluster + bulk-label unknown faces ----
  listFaces: (): Promise<Face[]> => api.get<Face[]>("/faces"),
  listFaceClusters: (eps?: number): Promise<FaceCluster[]> =>
    api.get<FaceCluster[]>(
      `/faces/clusters${eps != null ? `?eps=${eps}` : ""}`,
    ),
  /** Next fire time for each cron-mode When Event node in a graph.
   *
   *  Server-computed on purpose: the browser could evaluate the schedule itself, but then two
   *  implementations of "when does this fire" would have to agree forever — and the one the user
   *  believes is the one on screen. `next_fire_at` is null for a disabled graph, which is not going
   *  to fire at all. */
  cronNext: (
    graphId: string,
  ): Promise<{ enabled: boolean; nodes: { node: number; next_fire_at: string | null }[] }> =>
    api.get(`/workflows/${encodeURIComponent(graphId)}/cron`),
  /** Device Gate nodes with someone still deciding (#176). A node with nobody waiting is ABSENT,
   *  not present with a null — there is either a deadline to count down to or nothing to show. */
  deviceGatePending: (
    graphId: string,
  ): Promise<{ nodes: { node: number; expires_at: string }[] }> =>
    api.get(`/workflows/${encodeURIComponent(graphId)}/device-gate`),
  faceDetail: (id: string): Promise<FaceDetail> =>
    api.get<FaceDetail>(`/faces/${encodeURIComponent(id)}/detail`),
  /** On-demand facial landmarks (5 keypoints, in the crop's pixel coords) for the detail overlay. */
  faceLandmarks: (id: string): Promise<{ landmarks: [number, number][] }> =>
    api.get<{ landmarks: [number, number][] }>(
      `/faces/${encodeURIComponent(id)}/landmarks`,
    ),
  faceLayout: (
    opts: {
      eps?: number;
      scope?: "unknown" | "known" | "all";
      maxPerIsland?: number;
      maxIslands?: number;
      minSize?: number;
      spread?: number;
      since?: string;
      until?: string;
    } = {},
  ): Promise<CanvasLayout> => {
    const q = new URLSearchParams();
    if (opts.eps != null) q.set("eps", String(opts.eps));
    if (opts.scope) q.set("scope", opts.scope);
    if (opts.maxPerIsland != null)
      q.set("max_per_island", String(opts.maxPerIsland));
    if (opts.maxIslands != null) q.set("max_islands", String(opts.maxIslands));
    if (opts.minSize != null) q.set("min_size", String(opts.minSize));
    if (opts.spread != null) q.set("spread", String(opts.spread));
    if (opts.since) q.set("since", opts.since);
    if (opts.until) q.set("until", opts.until);
    const s = q.toString();
    return api.get<CanvasLayout>(`/faces/layout${s ? `?${s}` : ""}`);
  },
  /** A displayable crop URL (token-in-query so it works as `<img src>`). `bust` forces a reload. */
  faceImageUrl: (
    id: string,
    opts?: { box?: boolean; bust?: number },
  ): string => {
    const qs = new URLSearchParams();
    const mt = mediaToken();
    if (mt) qs.set("token", mt);
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
    await api.post<void>("/faces/reassign", {
      face_id: faceId,
      person_id: personId ?? null,
    });
  },
  ignoreFaces: async (faceIds: string[], ignored = true): Promise<void> => {
    await api.post<void>("/faces/ignore", { face_ids: faceIds, ignored });
  },
  /** HARD-delete faces (rows + MinIO crops) — for false positives / junk. Not recoverable. */
  deleteFaces: (faceIds: string[]): Promise<{ deleted: number }> =>
    api.post<{ deleted: number }>("/faces/delete", { face_ids: faceIds }),

  // ---- beacons / telegram -------------------------------------------------
  listBeacons: (): Promise<NotificationTarget[]> =>
    api.get<NotificationTarget[]>("/beacons"),
  /** Registered mobile push devices (card #139). Each device is its OWN Recipient, so the
   *  workflow recipient pickers merge these with the Telegram beacons. */
  listPushDevices: <
    T = { id: string; label: string; platform: string; enabled: boolean },
  >(): Promise<T[]> => api.get<T[]>("/push/devices"),
  /** Rename a phone — which renames a RECIPIENT, since the label is how a workflow addresses it.
   *  The server also repoints every graph that used the old name, and answers with how many it
   *  changed, so the UI can say so instead of leaving the user to wonder. */
  renamePushDevice: (id: string, label: string): Promise<{ workflows_updated: number }> =>
    api.patch<{ workflows_updated: number }>(`/push/devices/${encodeURIComponent(id)}`, { label }),
  deletePushDevice: async (id: string): Promise<void> => {
    await api.del<void>(`/push/devices/${encodeURIComponent(id)}`);
  },
  /** Send a test push — the phone's equivalent of the beacon test button. ALWAYS pass the label:
   *  the endpoint falls back to every enabled device when it is omitted, and a "test" that buzzes
   *  the whole household when someone meant to check one phone is a bad surprise. */
  testPushDevice: (label: string): Promise<{ sent: number }> =>
    api.post<{ sent: number }>("/push/devices/test", { label }),
  getTelegramConfig: (): Promise<TelegramConfigState> =>
    api.get<TelegramConfigState>("/telegram"),
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
  loadGraphs: <T = unknown>(opts?: {
    q?: string;
    group?: string;
  }): Promise<T[]> => {
    const qs = new URLSearchParams();
    if (opts?.q) qs.set("q", opts.q);
    if (opts?.group) qs.set("group", opts.group);
    const tail = qs.toString();
    return api.get<T[]>(`/workflows${tail ? `?${tail}` : ""}`);
  },
  runStates: <T = unknown>(): Promise<T> => api.get<T>("/workflows/runs"),
  nodeCatalog: <T = unknown>(): Promise<T> => api.get<T>("/workflows/catalog"),
  createGraph: (name: string): Promise<string> =>
    api.post<string>("/workflows", { name }),
  renameGraph: async (id: string, name: string): Promise<void> => {
    await api.patch<void>(`/workflows/${id}`, { name });
  },
  setGraphDescription: async (id: string, value: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/description`, { value });
  },
  setGraphGroup: async (id: string, value: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/group`, { value });
  },
  duplicateGraph: (id: string): Promise<string> =>
    api.post<string>(`/workflows/${id}/duplicate`),

  /** Templates a tenant can start from: its own + every global one (#102). */
  listTemplates: <T = unknown>(): Promise<T[]> =>
    api.get<T[]>("/workflows/templates"),
  /** Clone a template into the caller's tenant as a normal, disabled workflow; returns its id. */
  useTemplate: (id: string): Promise<string> =>
    api.post<string>(`/workflows/templates/${id}/use`),
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
  listDictionaries: (): Promise<Dictionary[]> =>
    api.get<Dictionary[]>("/dictionaries"),
  /** One dictionary by id. */
  getDictionary: (id: string): Promise<Dictionary> =>
    api.get<Dictionary>(`/dictionaries/${id}`),
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
    api.get<InstanceValues>(
      `/workflows/${id}/instance-values?key=${encodeURIComponent(key)}`,
    ),

  deleteGraph: async (id: string): Promise<void> => {
    await api.del<void>(`/workflows/${id}`);
  },
  // `setGraphTrigger` is gone with `PUT /workflows/:id/trigger`: a graph has one trigger, and a
  // schedule is config on a `When Event` node, saved with the graph.
  setGraphConcurrency: async (
    id: string,
    kind: string,
    max?: number,
  ): Promise<void> => {
    await api.put<void>(`/workflows/${id}/concurrency`, { kind, max });
  },
  /** What a graph runs one of (#95): "single" (default) or "per_camera". */
  setGraphInstanceScope: async (id: string, kind: string): Promise<void> => {
    await api.put<void>(`/workflows/${id}/instance-scope`, { kind });
  },
  setGraphEnabled: async (id: string, enabled: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/enabled`, { value: enabled });
  },
  setGraphRunOrphans: async (
    id: string,
    runOrphans: boolean,
  ): Promise<void> => {
    await api.put<void>(`/workflows/${id}/run-orphans`, { value: runOrphans });
  },
  setGraphDebug: async (id: string, debug: boolean): Promise<void> => {
    await api.put<void>(`/workflows/${id}/debug`, { value: debug });
  },
  setGraphViewport: async (
    id: string,
    x: number,
    y: number,
    zoom: number,
  ): Promise<void> => {
    await api.put<void>(`/workflows/${id}/viewport`, { x, y, zoom });
  },
  setEdgeVias: async (
    id: string,
    vias: Record<string, [number, number][]>,
  ): Promise<void> => {
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
    body: {
      nodes: unknown[];
      edges: unknown[];
      viewport?: unknown;
      edge_vias?: unknown;
      next_id?: number;
    },
  ): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/graph`, body);
  },
  addNode: (
    graphId: string,
    kind: string,
    x: number,
    y: number,
  ): Promise<number> =>
    api.post<number>(`/workflows/${graphId}/nodes`, { kind, x, y }),
  removeNode: async (graphId: string, nodeId: number): Promise<void> => {
    await api.del<void>(`/workflows/${graphId}/nodes/${nodeId}`);
  },
  moveNode: async (
    graphId: string,
    nodeId: number,
    x: number,
    y: number,
  ): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/move`, { x, y });
  },
  setNodeConfig: async (
    graphId: string,
    nodeId: number,
    config: unknown,
  ): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/config`, config);
  },
  setNodeMemoize: async (
    graphId: string,
    nodeId: number,
    memoize: boolean,
  ): Promise<void> => {
    await api.put<void>(`/workflows/${graphId}/nodes/${nodeId}/memoize`, {
      memoize,
    });
  },
  addEdge: async (
    graphId: string,
    from: number,
    fromPort: string,
    to: number,
    toPort: string,
  ): Promise<void> => {
    await api.post<void>(`/workflows/${graphId}/edges`, {
      from,
      from_port: fromPort,
      to,
      to_port: toPort,
    });
  },
  removeEdge: async (graphId: string, idx: number): Promise<void> => {
    await api.del<void>(`/workflows/${graphId}/edges/${idx}`);
  },
  /** Manually run a graph. `inputs` (optional) supplies per-launch values for the graph's input
   *  variables (#102), by variable name — they override the stored defaults for this run only. */
  runGraph: (id: string, inputs?: Record<string, string>): Promise<string> =>
    api.post<string>(
      `/workflows/${id}/run`,
      inputs && Object.keys(inputs).length ? { inputs } : undefined,
    ),

  // ---- workflow events + run history --------------------------------------
  listEvents: <T = unknown>(): Promise<T> =>
    api.get<T>("/workflows/event-registry"),
  createEvent: async (
    name: string,
    category?: string,
    description?: string,
    fields?: { name: string; type: string }[],
  ): Promise<void> => {
    await api.post<void>("/workflows/event-registry", {
      name,
      category,
      description,
      fields,
    });
  },
  deleteEvent: async (name: string): Promise<void> => {
    await api.del<void>(
      `/workflows/event-registry/${encodeURIComponent(name)}`,
    );
  },
  listEventCategories: <T = unknown>(): Promise<T> =>
    api.get<T>("/workflows/event-categories"),
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
    const qs = new URLSearchParams({
      graph: String(graph),
      node: String(node),
    });
    const mt = mediaToken();
    if (mt) qs.set("token", mt);
    if (bust != null) qs.set("t", String(bust));
    return `${API_BASE}/workflows/runs/frame?${qs.toString()}`;
  },
  nodeStateUrl: (graph: string, node: number): string => {
    const qs = new URLSearchParams({
      graph: String(graph),
      node: String(node),
    });
    const mt = mediaToken();
    if (mt) qs.set("token", mt);
    return `${API_BASE}/workflows/runs/node-state?${qs.toString()}`;
  },
};
