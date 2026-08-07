// Typed calls against the control plane. Mirrors the shapes in `sentinel-app/src/lib/data.ts` and
// `types.ts` — only the fields the phone actually uses.
import { api } from "./session";

/** A saved camera (`app_core::devices::Device`).
 *
 *  `xaddr` is REQUIRED, matching the Rust struct — it carries no `#[serde(default)]`, so a payload
 *  without it is rejected with "missing field `xaddr`". It was optional here, which let a caller
 *  omit it and only find out from the server at run time. Empty string is the right value for a
 *  camera with no ONVIF endpoint. */
export type Device = {
  id?: string;
  ip: string;
  xaddr: string;
  name: string;
  manufacturer?: string | null;
  model?: string | null;
};

/** A clip (`sentinel-control::store::RecordingRow`). */
export type Recording = {
  id: number;
  camera_ip: string;
  camera_name?: string | null;
  clip_id: string;
  friendly_name: string;
  duration_secs: number;
  size_bytes: number;
  started_at: string;
  /** `pending` (edge only) | `uploading` | `uploaded` (durable in R2) | `failed`. */
  upload_state: string;
  deleting?: boolean;
};

export type RecordingPage = { total: number; items: Recording[] };

export const listDevices = () => api<Device[]>("/devices");

/** Create or update a camera. Note the cloud STRIPS `rtsp_url` and credentials from this payload by
 *  design — they live only on the edge — so a stream URL has to travel via [`setCredentials`]. */
export const saveDevice = (device: Device) =>
  api<void>("/devices", { method: "PUT", body: JSON.stringify(device) });

/** Relay a camera's credentials — and, for a source with no ONVIF, its explicit stream URL — to the
 *  tenant's edge. Held in control's memory until the edge collects them, then discarded; nothing
 *  here is persisted, because an RTSP URL routinely embeds credentials.
 *
 *  This is how the phone's own camera becomes a real camera (#175): `rtspUrl` is `device:front` or
 *  `device:back`, which tells the edge to expect frames pushed in by the host app rather than
 *  something to dial. */
export const setCredentials = (
  ip: string,
  username: string,
  password: string,
  rtspUrl?: string,
) =>
  api<void>(`/devices/${encodeURIComponent(ip)}/credentials`, {
    method: "POST",
    body: JSON.stringify({ username, password, ...(rtspUrl ? { rtsp_url: rtspUrl } : {}) }),
  });

export const listRecordings = (limit = 50) =>
  api<RecordingPage>(`/recordings?limit=${limit}&sort=started_at&dir=desc`);

/** Presigned R2 URL, valid for an hour. 409 when the clip is still edge-only. */
export const recordingUrl = (id: number) => api<{ url: string }>(`/recordings/${id}/url`);

/** Stream name for an IP — must match the edge (`app_core::stream_names::stream_name`):
 *  `cam_<ip with dots→underscores>`, plus `_hd` for the main (high-definition) variant. */
export function streamName(ip: string, hd: boolean): string {
  return "cam_" + ip.replaceAll(".", "_") + (hd ? "_hd" : "");
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatDuration(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}
