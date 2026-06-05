export type Transport = "tcp" | "udp" | "auto";

export type Settings = {
  master: number;
  voice: number;
  ui: number;
  ambient: number;
  voice_enabled: boolean;
  typing_speed: number;
  stt_autosend: boolean;
  transport: Transport;
};

/** A saved camera (matches `app_core::devices::Device`). */
export type Device = {
  ip: string;
  xaddr: string;
  name: string;
  manufacturer?: string | null;
  model?: string | null;
  username?: string | null;
  password?: string | null;
  rtsp_url?: string | null;
};

/** A camera that answered a discovery probe (matches `app_core::onvif::DiscoveredDevice`). */
export type DiscoveredDevice = {
  xaddr: string;
  ip: string;
  name: string;
  manufacturer?: string | null;
  model?: string | null;
};

/** A playable go2rtc stream for the Live screen (matches `StreamInfo` in src-tauri). */
export type StreamInfo = {
  ip: string;
  /** go2rtc stream name, e.g. `cam_192_168_1_50`. */
  name: string;
  /** go2rtc's ready-made WebRTC player URL. */
  player_url: string;
};

export const DEFAULT_SETTINGS: Settings = {
  master: 0.8,
  voice: 0.8,
  ui: 0.6,
  ambient: 0.4,
  voice_enabled: true,
  typing_speed: 4,
  stt_autosend: false,
  transport: "auto",
};
