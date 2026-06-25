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
  // AI
  describe_image: portColor.dictionary, ask_ai: portColor.dictionary,
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
