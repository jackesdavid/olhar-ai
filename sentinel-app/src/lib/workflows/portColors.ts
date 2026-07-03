export type PortType =
  | "text" | "num" | "bool" | "image" | "recipient" | "dictionary" | "file" | "exec"
  | "text_array" | "num_array" | "person_array" | "face_array" | "person" | "face" | "camera" | "bbox" | "any";

export const portColor: Record<PortType, string> = {
  text: "#60a5fa",
  num: "#34d399",
  bool: "#fbbf24",
  image: "#f472b6",
  recipient: "#a78bfa",
  dictionary: "#2dd4bf",
  file: "#fb923c",
  exec: "#f4f4f5",
  text_array: "#93c5fd",
  num_array: "#fb923c",
  person_array: "#f0abfc",
  face_array: "#fda4af",
  person: "#e879f9",
  face: "#fb7185",
  camera: "#38bdf8",
  bbox: "#f59e0b",
  any: "#94a3b8",
};

export function typeColor(t: string): string {
  return (portColor as Record<string, string>)[t] ?? portColor.any;
}

// Fixed, theme-independent hues that live alongside the port palette so the editor
// carries NO inline colour literals. These are categorical identity (like the port
// types) — they do not flip with light/dark.
export const FALLBACK_PORT_COLOR = "#52796f"; // port with no resolvable type
export const COMMENT_DEFAULT_COLOR = "#facc15"; // sticky-note default fill
export const EVENT_HEADER_COLOR = "#f43f5e"; // emit/on-event nodes POP in red
export const DEFAULT_CATEGORY_COLOR = "#64748b"; // Flow / utility fallback (slate)

// Node-category → header tint. Keys are the exact `category()` strings from the Rust
// catalog (crates/app-core/src/workflows/domain.rs). Values reuse the fixed port palette
// where a category maps cleanly onto a type family.
export const CATEGORY_COLOR: Record<string, string> = {
  Cameras: portColor.camera, // sky
  Vision: portColor.person, // violet/magenta
  AI: "#818cf8", // indigo — the AI family's own identity (not borrowed from a port type)
  Alerts: portColor.bool, // amber
  Recipients: portColor.recipient, // purple
  Text: portColor.num, // green
  Network: "#22d3ee", // cyan
  Control: "#94a3b8", // slate
  Logic: "#94a3b8", // slate
  Presence: "#4ade80", // green (matches record_presence)
  Recording: "#ef4444", // red — a hot "REC" identity, distinct from every other family
  Debug: "#a1a1aa", // zinc
  Utility: "#94a3b8", // slate
  Variables: "#94a3b8", // slate (get/set actually take their variable's type colour)
  Telegram: "#229ed9", // Telegram brand blue — the bot-reply family
};

// Maps a workflow node kind slug to its category colour — same logic as the
// editor's headerColor() but statically derived so it works outside the editor.
const NODE_KIND_COLOR: Record<string, string> = {
  // Cameras
  camera_snapshot: portColor.camera, get_current_frame: portColor.camera,
  get_camera_by_name: portColor.camera, break_camera: portColor.camera,
  get_image_size: portColor.camera,
  // Vision
  motion_detection: portColor.person, motion_watch: portColor.person,
  person_detection: portColor.person, object_detection: portColor.person,
  break_person_detection: portColor.person, break_detection: portColor.person,
  break_face_detection: portColor.person, recognize_person: portColor.person,
  draw_bbox: portColor.person, blur_outside_bbox: portColor.person,
  // AI (indigo — the family's own identity, incl. the AI-powered AI Switch router)
  describe_image: "#818cf8", ask_ai: "#818cf8", ai_answer: "#818cf8", ai_switch: "#818cf8",
  // Telegram (bot reply)
  reply_to_sender: "#229ed9", reply_with_recording: "#229ed9",
  // Alerts
  get_last_alert: portColor.bool, create_alert: portColor.bool, deliver_alert: portColor.bool,
  // Recipients
  get_recipient: portColor.recipient, notify_recipient: portColor.recipient,
  // Text / Network
  format: portColor.num, num_to_text: portColor.num, bool_to_text: portColor.num,
  text_compare: portColor.num,
  http_post: "#22d3ee",
  // Presence
  record_presence: "#4ade80",
  // Events (stand out in red)
  emit_event: "#f43f5e", on_event: "#f43f5e",
};

export function nodeKindColor(slug: string | null | undefined): string {
  if (!slug) return "#64748b";
  return NODE_KIND_COLOR[slug] ?? "#64748b"; // slate for Flow/utility nodes
}
