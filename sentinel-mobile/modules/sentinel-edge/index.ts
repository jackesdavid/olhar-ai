// JS binding to the native SentinelEdge module (card #144). Present only on a device build that
// linked the Rust edge lib; guarded so the managed/Expo-Go path degrades instead of crashing.
import { requireOptionalNativeModule } from "expo-modules-core";

const Native = requireOptionalNativeModule("SentinelEdge");

/** Is the embedded edge available (a device build that linked libsentinel_edge.a)? */
export const hasEmbeddedEdge = Native != null;

/** Boot the embedded edge ENROLLED with this phone's box token + control URL (#143/#144), so it runs
 *  the full edge — pulls its cameras, records, reports health — and binds localhost:8788. */
export async function startEdge(boxToken: string, controlUrl: string): Promise<void> {
  if (!Native) throw new Error("embedded edge not available in this build");
  await Native.startEdge(boxToken, controlUrl);
}

/** Request the embedded edge to stop. */
export async function stopEdge(): Promise<void> {
  if (!Native) return;
  await Native.stopEdge();
}

/** Which lens the phone captures with. */
export type DeviceFacing = "front" | "back";

/** The `rtsp_url` a device-camera entry must carry for the edge to treat it as host-fed (#175).
 *  It is a URL scheme rather than a flag because the frame hub only starts for a device that HAS a
 *  source — and this one does, it just is not dialable. */
export const deviceCameraUrl = (facing: DeviceFacing): string => `device:${facing}`;

/** Start feeding the phone's own camera into the running edge as an ordinary camera (#175 Phase 1).
 *
 *  `cameraId` must match a device in the tenant's list whose `rtsp_url` is `deviceCameraUrl(...)`;
 *  that entry is what makes the frame hub start for it. Everything after the frame — motion, YuNet,
 *  blur/yaw/pitch, the `face_detected` event — is the same code a wall camera runs.
 *
 *  Foreground only. iOS restricts camera access in the background, so capture ends when the app
 *  leaves the foreground; treat it as a session the user starts, not a service. */
export async function startDeviceCamera(
  cameraId: string,
  facing: DeviceFacing,
  fps = 5,
  quality = 0.6,
): Promise<void> {
  if (!Native) throw new Error("embedded edge not available in this build");
  await Native.startDeviceCamera(cameraId, facing, fps, quality);
}

export async function stopDeviceCamera(): Promise<void> {
  if (!Native) return;
  await Native.stopDeviceCamera();
}

export function isDeviceCameraRunning(): boolean {
  return Native?.isDeviceCameraRunning?.() ?? false;
}
