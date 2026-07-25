// User-tunable face-capture parameters (card #143). These were hardcoded constants in the capture
// screen; this makes them adjustable and persists them on the device. Not secrets, but stored via
// SecureStore anyway since it is the storage the app already depends on — the blob is tiny.
import * as SecureStore from "expo-secure-store";

const KEY = "sentinel.captureSettings";

/** How the capture screen fires. Continuous snaps on an interval; manual snaps on a tap. */
export type CaptureMode = "continuous" | "manual";

export type CaptureSettings = {
  mode: CaptureMode;
  /** Interval between snaps in continuous mode, milliseconds. */
  intervalMs: number;
  /** Longest edge of the uploaded JPEG, pixels — the downscale before upload. */
  maxWidth: number;
  /** JPEG quality 0..1 for both the capture and the re-encode. */
  quality: number;
  /** Which camera to use. */
  facing: "front" | "back";
};

export const DEFAULT_SETTINGS: CaptureSettings = {
  mode: "continuous",
  intervalMs: 1800,
  maxWidth: 720,
  quality: 0.6,
  facing: "front",
};

/** The choices the settings screen offers, so the UI and the validation share one source. */
export const INTERVAL_OPTIONS = [
  { label: "1s", value: 1000 },
  { label: "1.8s", value: 1800 },
  { label: "3s", value: 3000 },
  { label: "5s", value: 5000 },
];
export const RESOLUTION_OPTIONS = [
  { label: "480p", value: 480 },
  { label: "720p", value: 720 },
  { label: "1080p", value: 1080 },
];
export const QUALITY_OPTIONS = [
  { label: "Low", value: 0.4 },
  { label: "Medium", value: 0.6 },
  { label: "High", value: 0.8 },
];

/** Load saved settings, falling back to defaults for anything missing or corrupt — a bad blob must
 *  never leave the capture screen unusable. */
export async function loadSettings(): Promise<CaptureSettings> {
  try {
    const raw = await SecureStore.getItemAsync(KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<CaptureSettings>;
    return { ...DEFAULT_SETTINGS, ...parsed };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export async function saveSettings(s: CaptureSettings): Promise<void> {
  await SecureStore.setItemAsync(KEY, JSON.stringify(s));
}
