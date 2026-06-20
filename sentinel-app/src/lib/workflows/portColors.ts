export type PortType =
  | "text" | "num" | "bool" | "image" | "recipient" | "dictionary" | "file" | "exec"
  | "text_array" | "num_array" | "person_array" | "face_array" | "person" | "face" | "camera" | "any";

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
  num_array: "#6ee7b7",
  person_array: "#f0abfc",
  face_array: "#fda4af",
  person: "#e879f9",
  face: "#fb7185",
  camera: "#38bdf8",
  any: "#94a3b8",
};

export function typeColor(t: string): string {
  return (portColor as Record<string, string>)[t] ?? portColor.any;
}
