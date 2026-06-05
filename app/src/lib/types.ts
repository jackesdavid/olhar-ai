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
