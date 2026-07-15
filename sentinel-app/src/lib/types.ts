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
  cameras_always_warm: boolean;
  /** Max ring buffer size in seconds; also the upper bound for per-recording pre-roll. */
  pre_roll_secs: number;
  /** MOG2 varThreshold for motion detection. Lower = more sensitive (default 16). */
  motion_var_threshold: number;
  /** Maximum recording storage in MB before old clips are auto-evicted (default 10000 = ~10 GB). */
  max_storage_mb: number;
  /** Directory on the edge where .mp4 clips are saved (default /opt/sentinel-edge/recordings). */
  recordings_path: string;
  /**
   * Delete a clip's local file once the cloud confirms it's durable — the box's disk becomes a
   * transient spool instead of an archive. Requires a plan that sells cloud recording; without
   * one nothing is ever uploaded and the local file is the only copy.
   */
  delete_local_after_upload: boolean;
};

/** A saved camera (matches `app_core::devices::Device`). */
export type Device = {
  /** Stable ONVIF device id (survives IP/DHCP changes); "" if unknown. */
  id?: string;
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
  /** Stable ONVIF device id (WS-Addressing EndpointReference). */
  id?: string;
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

export type AlertLevel = "debug" | "info" | "warn" | "error";

/** A delivery record of an alert to a beacon (data only until unit 017). */
export type Delivery = {
  target: string;
  chat_id: number;
  at: string;
  ok: boolean;
};

/** A single alert (matches `app_core::alerts::Alert`). */
export type Alert = {
  id: number;
  level: AlertLevel;
  message: string;
  room_id: number | null;
  room_name: string | null;
  source: string;
  created_at: string;
  deliveries: Delivery[];
};

/** Aggregate alert counts for the dashboard (matches `AlertStats` in src-tauri). */
export type AlertStats = {
  total: number;
  today: number;
  /** `[label, count]` per level, e.g. `["DEBUG", 0]`. */
  by_level: [string, number][];
  undelivered: number;
};

/** A paired Telegram beacon (matches `app_core::notification::NotificationTarget`). */
export type NotificationTarget = {
  label: string;
  chat_id: number;
  enabled: boolean;
};

/** Telegram config state for the Recipients screen (matches `TelegramState`).
 * The bot token is never returned — only whether it's set via the global provider. */
export type TelegramConfigState = {
  provider_configured: boolean;
  pairing_code: string;
  bot_username?: string;
};

/** Global Telegram provider state (master-only, matches `TelegramProviderState`). */
export type TelegramProviderState = {
  /** The GLOBAL shared bot is set — not "does notification work at all" (see tenant_bot_count). */
  configured: boolean;
  bot_username?: string;
  /** Tenants running their own bot instead of the global one (card #73). */
  tenant_bot_count: number;
};

/** A managed person (matches `app_core::persons::Person`). The hierarchy is
 * expressed by `parent_id` (the superior); `permissions` are the actions this
 * person may perform over the people beneath it. Face biometrics live in their own
 * table (`PersonEmbedding`) — a person is "enrolled" when they have ≥1 approved row. */
export type Person = {
  id: string;
  name: string;
  parent_id: string | null;
  recipient_label: string | null;
  permissions: string[];
};

/** One enrolled face embedding (matches `app_core::persons::PersonEmbedding`). The
 * raw `embedding` vector is returned only to render the privacy-safe point-cloud
 * avatar — the source photo is never stored. */
export type PersonEmbedding = {
  id: string;
  person_id: string;
  approved: boolean;
  source: string;
  created_at: string;
  embedding: number[];
};

/** A recognition result (matches `app_core::persons::RecognitionResult`). */
export type RecognitionResult = {
  person_id: string;
  name: string;
  confidence: number;
};

/** A recognition probe result: the query embedding (for the target point-cloud) plus
 * the best match (matches the Tauri `recognize_face` command). */
export type RecognizeDetail = {
  query: number[];
  match: RecognitionResult | null;
};

/** A camera assigned to a room (matches `app_core::map::RoomCamera`).
 * Positioned by an offset (`dx,dy`) in cells from the room's top-left.
 * `rot` is the yaw in degrees (0 = facing +y / "down" on the plan) used to
 * orient the field-of-view wedge; optional + defaulted for back-compat. */
export type RoomCamera = {
  ip: string;
  dx: number;
  dy: number;
  rot?: number;
  /** Field-of-view angle in degrees (wedge width). */
  fov?: number;
  /** View distance in grid units (wedge length). */
  range?: number;
};

/** One room on the floor plan (matches `app_core::map::MapRoom`).
 * Axis-aligned rectangle on an integer grid, y-down/top-left origin. */
export type MapRoom = {
  id: number;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** Rotation of the room rectangle in degrees (about its centre). */
  rot?: number;
  cameras: RoomCamera[];
};

/** Wall render style — independent of thickness.
 * solid = filled raised wall; wireframe = flat box outline;
 * dashed / dotted = centerline dash or dot; line = solid 1px flat line. */
export type WallStyle = "solid" | "wireframe" | "dashed" | "dotted" | "line";

/** A free-form wall (matches `app_core::map::MapWall`). */
export type MapWall = {
  id: number;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  thickness: number;
  style?: WallStyle;
  /** Hex color override (e.g. "#ff0000"). Undefined = use palette default. */
  color?: string;
};

/** A placeable furniture / landmark marker (matches `app_core::map::MapMarker`),
 * rendered as a flat SVG glyph on the floor. `icon` keys the icon library. */
export type MapMarker = {
  id: number;
  icon: string;
  x: number;
  y: number;
  rot: number;
  scale: number;
  label?: string | null;
};

/** A text label placed on the floor plan (matches `app_core::map::MapText`). */
export type MapText = {
  id: number;
  text: string;
  x: number;
  y: number;
  size?: number;   // font size in world units, default 0.5
  color?: string;  // default white
  rot?: number;    // degrees
};

/** Stroke pattern for shape outlines. */
export type StrokeStyle = "solid" | "dashed" | "dotted";

/** Shape annotation on the floor plan (matches `app_core::map::MapShape`).
 * kind: "rect" | "triangle" | "circle" */
export type ShapeKind = "rect" | "triangle" | "circle";
export type MapShape = {
  id: number;
  kind: ShapeKind;
  x: number;      // center x (rect/circle); unused for triangle
  y: number;      // center y
  w?: number;     // rect width
  h?: number;     // rect height
  r?: number;     // circle radius
  pts?: [number, number][];  // triangle: [[x0,y0],[x1,y1],[x2,y2]]
  color?: string;            // stroke color
  fill?: string;             // fill color (undefined = no fill)
  strokeStyle?: StrokeStyle;
  rot?: number;              // degrees
};

/** The whole floor-plan document exchanged with `/map` (matches `app_core::map::MapDoc`). */
export type MapDoc = {
  rooms: MapRoom[];
  walls: MapWall[];
  markers: MapMarker[];
  texts: MapText[];
  shapes: MapShape[];
};

/** What a presence detection was (matches `app_core::presence::Category`). */
export type PresenceCategory =
  | "person"
  | "known_person"
  | "car"
  | "cat"
  | "dog"
  | "unknown";

/** A recorded presence event (matches `app_core::presence::Presence`). Written by
 * the RecordPresence workflow node; pulsed live on the map. */
export type Presence = {
  id: number;
  category: PresenceCategory;
  label?: string | null;
  room_id?: number | null;
  room_name?: string | null;
  camera?: string | null;
  /** How long the pulse stays on the map, in seconds. */
  duration_secs?: number;
  source: string;
  created_at: string;
  /** Normalised horizontal position within camera FOV (-0.5 left, 0 centre, 0.5 right). */
  detect_x?: number | null;
  /** Normalised depth within camera range (0 near camera, 1 far edge). */
  detect_depth?: number | null;
};

/** A live presence push over SSE (the `presence` event from sentinel-flow). */
export type PresenceEvent = {
  category: PresenceCategory;
  room_id?: number | null;
  room_name?: string | null;
  label?: string | null;
  camera?: string | null;
  /** How long the pulse should stay on the map, in seconds (RecordPresence config). */
  duration_secs?: number;
  created_at: string;
  /** Normalised horizontal position within camera FOV (-0.5 left, 0 centre, 0.5 right). */
  detect_x?: number | null;
  /** Normalised depth within camera range (0 near camera, 1 far edge). */
  detect_depth?: number | null;
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
  cameras_always_warm: false,
  pre_roll_secs: 10,
  motion_var_threshold: 16,
  max_storage_mb: 10000,
  recordings_path: "/opt/sentinel-edge/recordings",
  delete_local_after_upload: false,
};
