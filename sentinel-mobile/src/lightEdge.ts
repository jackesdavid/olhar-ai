// Light-edge face capture (card #142): the phone as an *on-demand* edge. It registers once as a
// lightweight box, then — only while the capture screen is open — pushes detected faces into the
// same cloud pipeline an appliance feeds (embed → recognize → StoreFace). Foreground-only by design,
// which is what keeps it inside iOS's rules.
//
// Two different credentials, deliberately:
//   • registration uses the USER's JWT (`api()`), because self-register is a user action;
//   • face push uses the BOX TOKEN it hands back, because `POST /config/face` authenticates a box,
//     not a user — exactly as an appliance does.
import * as SecureStore from "expo-secure-store";

import { CONTROL_BASE } from "./config";
import { saveDevice, setCredentials } from "./data";
import { api } from "./session";

const BOX_TOKEN_KEY = "sentinel.lightedge.boxToken";
const CAMERA_IP_KEY = "sentinel.lightedge.cameraIp";

export type LightEdge = {
  /** The box token — the bearer on every face push. Stored in the keychain, never AsyncStorage. */
  boxToken: string;
  /** Stamped on every face as `camera_ip`; a workflow's `When Event(face_detected)` filters on it. */
  cameraIp: string;
};

type SelfRegisterResp = { box_id: string; camera_ip: string; token: string };

/** One face to push — the shape `sentinel-control`'s `FaceIn` (`POST /config/face`) expects. */
export type FaceCapture = {
  /** Canonical xyxy in source pixels. */
  bbox: [number, number, number, number];
  /** 0..100. */
  confidence: number;
  /** Face box width in source pixels — the face-size signal the pipeline gates on. */
  faceWidth: number;
  /** YuNet-order 5 landmarks, flattened `[x0,y0,…]`, source pixels (#122). */
  kps: number[];
  /** The face crop as base64 JPEG (no data: prefix) — becomes `frame_b64`. */
  frameB64: string;
};

/** The stored light-edge identity, or null if this phone has never registered. */
export async function restore(): Promise<LightEdge | null> {
  const [boxToken, cameraIp] = await Promise.all([
    SecureStore.getItemAsync(BOX_TOKEN_KEY),
    SecureStore.getItemAsync(CAMERA_IP_KEY),
  ]);
  return boxToken && cameraIp ? { boxToken, cameraIp } : null;
}

/** Is this stored box identity still one the server knows?
 *
 *  `/config/cameras` is the box's own endpoint and answers 401 "unknown box token" for an identity
 *  that no longer exists — the cheapest honest check, and it exercises the exact credential the
 *  edge will boot with. A network failure is NOT treated as invalid: dropping a good identity
 *  because the wifi blipped would spawn a duplicate box every time the phone is offline. */
async function stillValid(edge: LightEdge): Promise<boolean> {
  try {
    const r = await fetch(`${CONTROL_BASE}/config/cameras`, {
      headers: { authorization: `Bearer ${edge.boxToken}` },
      signal: AbortSignal.timeout(8000),
    });
    return r.status !== 401;
  } catch {
    return true; // unreachable ≠ rejected
  }
}

/** Register this phone as a light-edge box, or return the existing identity if already registered.
 *
 *  Idempotent on the CLIENT: the server is only called when there is no usable stored token, so a
 *  phone does not spawn a new box on every launch.
 *
 *  The stored identity is VERIFIED, not trusted. The keychain outlives the server row — delete the
 *  box from the Boxes screen (or wipe the tenant's boxes) and the phone keeps a token nothing
 *  accepts. Everything then fails in the most confusing way available: registering a camera still
 *  works (that uses the USER's JWT), so a device row appears and looks right, while the edge boots
 *  with a rejected token, pulls no cameras, starts no frame hub, and reports nothing at all. That is
 *  exactly what happened on the first end-to-end run of #175: a `phone-227a7d58` device sitting in
 *  the tenant with no box `227a7d58` behind it. */
export async function ensureRegistered(name: string): Promise<LightEdge> {
  const existing = await restore();
  if (existing && (await stillValid(existing))) return existing;
  if (existing) await forget(); // orphaned by a server-side delete — start over

  const resp = await api<SelfRegisterResp>("/boxes/self-register", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
  await Promise.all([
    SecureStore.setItemAsync(BOX_TOKEN_KEY, resp.token),
    SecureStore.setItemAsync(CAMERA_IP_KEY, resp.camera_ip),
  ]);
  return { boxToken: resp.token, cameraIp: resp.camera_ip };
}

/** Register the phone's OWN camera as a real camera on this box (#175 Phase 1).
 *
 *  Two calls, and the split is forced by the credential invariant: `PUT /devices` creates the row
 *  but control deliberately STRIPS `rtsp_url` from it (camera URLs never touch the cloud DB), so the
 *  URL has to travel the memory-only credential channel instead. The edge collects it on its next
 *  config pull — which `setCredentials` wakes immediately — and from then on the frame hub runs for
 *  this camera exactly as it does for a wall camera.
 *
 *  `self_register` documents that it creates no `devices` row because "that table is full-replaced
 *  by the appliance's camera sync, which would clobber a phone camera". That is no longer true: no
 *  edge endpoint calls `devices::repo::save`, discovery lands in the separate `discovered_cameras`
 *  table, and the only edge-driven write (`move_ip`) matches on an ONVIF id a phone camera does not
 *  have. The remaining full-replaces are the user's own upsert/remove, which rewrite the whole list
 *  including this row.
 *
 *  Returns the camera key to pass to `startDeviceCamera`. */
export async function registerDeviceCamera(
  edge: LightEdge,
  facing: "front" | "back",
): Promise<string> {
  const ip = edge.cameraIp; // stable + unique per phone, derived from the box id server-side
  // `xaddr` empty, not absent: there is no ONVIF endpoint on a phone lens, but the field is
  // required by the Rust `Device` (it has no serde default).
  await saveDevice({ ip, xaddr: "", name: `Phone — ${facing} camera` });
  await setCredentials(ip, "", "", `device:${facing}`);
  return ip;
}

/** Forget this phone's light-edge identity (the box row stays server-side until deleted there). */
export async function forget(): Promise<void> {
  await Promise.all([
    SecureStore.deleteItemAsync(BOX_TOKEN_KEY),
    SecureStore.deleteItemAsync(CAMERA_IP_KEY),
  ]);
}

/** Push one captured face. Authenticated by the BOX token (not the user JWT) — the same bearer an
 *  appliance presents to `/config/face`. Best-effort per face: a dropped push is one missed frame,
 *  not a failure worth interrupting the capture session for. Returns whether it was accepted. */
export async function pushFace(edge: LightEdge, face: FaceCapture): Promise<boolean> {
  const body = JSON.stringify({
    camera_ip: edge.cameraIp,
    bbox: face.bbox,
    confidence: face.confidence,
    face_width: face.faceWidth,
    kps: face.kps,
    frame_b64: face.frameB64,
  });
  const r = await fetch(`${CONTROL_BASE}/config/face`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${edge.boxToken}` },
    body,
  });
  return r.ok;
}
