// MapStore — the editor's working copy of the floor plan: room zones, free-form
// walls, furniture/landmark markers, text labels, shape annotations, plus the
// device list, selection, undo/redo, and the dirty/saving flags.
//
// Mutations re-assign the relevant array (so Svelte 5 reactivity fires) and set
// `dirty`. Discrete actions checkpoint history first; continuous gestures (drag,
// sliders) checkpoint once at the start via `beginGesture()`. The whole plan saves
// as one { rooms, walls, markers, texts, shapes } document.
import { data } from "$lib/data";
import type { Device, MapDoc, MapMarker, MapRoom, MapShape, MapText, MapWall } from "$lib/types";
import { History } from "./history.svelte";

const DEFAULT_ROOM_W = 6;
const DEFAULT_ROOM_H = 4;

function clampInt(v: number, min: number): number {
  return Number.isFinite(v) ? Math.max(min, Math.round(v)) : min;
}

export type SelKind = "room" | "wall" | "marker" | "text" | "shape";
export type Selection = { kind: SelKind; id: number } | null;

export class MapStore {
  rooms = $state<MapRoom[]>([]);
  walls = $state<MapWall[]>([]);
  markers = $state<MapMarker[]>([]);
  texts = $state<MapText[]>([]);
  shapes = $state<MapShape[]>([]);
  devices = $state<Device[]>([]);

  sel = $state<Selection>(null);
  loading = $state(true);
  saving = $state(false);
  dirty = $state(false);
  canUndo = $state(false);
  canRedo = $state(false);

  private history = new History();

  // ---- selection ----------------------------------------------------------
  selectedRoom = $derived(
    this.sel?.kind === "room" ? (this.rooms.find((r) => r.id === this.sel!.id) ?? null) : null,
  );
  selectedWall = $derived(
    this.sel?.kind === "wall" ? (this.walls.find((w) => w.id === this.sel!.id) ?? null) : null,
  );
  selectedMarker = $derived(
    this.sel?.kind === "marker" ? (this.markers.find((m) => m.id === this.sel!.id) ?? null) : null,
  );
  selectedText = $derived(
    this.sel?.kind === "text" ? (this.texts.find((t) => t.id === this.sel!.id) ?? null) : null,
  );
  selectedShape = $derived(
    this.sel?.kind === "shape" ? (this.shapes.find((s) => s.id === this.sel!.id) ?? null) : null,
  );
  selectedRoomId = $derived(this.sel?.kind === "room" ? this.sel.id : null);
  selectedWallId = $derived(this.sel?.kind === "wall" ? this.sel.id : null);
  selectedMarkerId = $derived(this.sel?.kind === "marker" ? this.sel.id : null);
  selectedTextId = $derived(this.sel?.kind === "text" ? this.sel.id : null);
  selectedShapeId = $derived(this.sel?.kind === "shape" ? this.sel.id : null);

  selectRoom(id: number | null): void {
    this.sel = id === null ? null : { kind: "room", id };
  }
  selectWall(id: number): void {
    this.sel = { kind: "wall", id };
  }
  selectMarker(id: number): void {
    this.sel = { kind: "marker", id };
  }
  selectText(id: number): void {
    this.sel = { kind: "text", id };
  }
  selectShape(id: number): void {
    this.sel = { kind: "shape", id };
  }
  clearSelection(): void {
    this.sel = null;
  }

  // ---- devices ------------------------------------------------------------
  deviceByIp = $derived(new Map(this.devices.map((d) => [d.ip, d])));
  deviceName(ip: string): string {
    return this.deviceByIp.get(ip)?.name ?? ip;
  }
  get assignable(): Device[] {
    const sel = this.selectedRoom;
    if (!sel) return [];
    return this.devices.filter((d) => !sel.cameras.some((c) => c.ip === d.ip));
  }

  // ---- history ------------------------------------------------------------
  private doc(): MapDoc {
    return {
      rooms: this.rooms,
      walls: this.walls,
      markers: this.markers,
      texts: this.texts,
      shapes: this.shapes,
    };
  }
  private apply(doc: MapDoc): void {
    this.rooms = doc.rooms;
    this.walls = doc.walls;
    this.markers = doc.markers;
    this.texts = doc.texts ?? [];
    this.shapes = doc.shapes ?? [];
    this.dirty = true;
  }
  private syncHistory(): void {
    this.canUndo = this.history.canUndo;
    this.canRedo = this.history.canRedo;
  }
  /** Record a restore point before a mutation (or at a gesture's start). */
  checkpoint(): void {
    this.history.push(this.doc());
    this.dirty = true;
    this.syncHistory();
  }
  /** Alias used by the scene at drag pointerdown — one checkpoint per gesture. */
  beginGesture(): void {
    this.checkpoint();
  }
  undo(): void {
    const d = this.history.undo(this.doc());
    if (d) this.apply(d);
    this.syncHistory();
  }
  redo(): void {
    const d = this.history.redo(this.doc());
    if (d) this.apply(d);
    this.syncHistory();
  }

  private nextId(items: { id: number }[]): number {
    return items.reduce((m, x) => Math.max(m, x.id), -1) + 1;
  }

  // ---- load / save --------------------------------------------------------
  async load(): Promise<void> {
    this.loading = true;
    try {
      const [doc, ds] = await Promise.all([data.loadMap(), data.listDevices()]);
      this.rooms = doc.rooms ?? [];
      this.walls = doc.walls ?? [];
      this.markers = doc.markers ?? [];
      this.texts = doc.texts ?? [];
      this.shapes = doc.shapes ?? [];
      this.devices = ds;
      this.sel = this.rooms.length > 0 ? { kind: "room", id: this.rooms[0].id } : null;
      this.dirty = false;
      this.history = new History();
      this.syncHistory();
    } finally {
      this.loading = false;
    }
  }

  async save(): Promise<void> {
    this.saving = true;
    try {
      await data.saveMap(this.doc());
      this.dirty = false;
    } finally {
      this.saving = false;
    }
  }

  // ---- rooms --------------------------------------------------------------
  addRoom(): MapRoom {
    this.checkpoint();
    const id = this.nextId(this.rooms);
    const room: MapRoom = {
      id,
      name: `Room ${id + 1}`,
      x: 0,
      y: 0,
      w: DEFAULT_ROOM_W,
      h: DEFAULT_ROOM_H,
      cameras: [],
    };
    this.rooms = [...this.rooms, room];
    this.sel = { kind: "room", id };
    return room;
  }

  deleteRoom(id: number): void {
    this.checkpoint();
    this.rooms = this.rooms.filter((r) => r.id !== id);
    if (this.sel?.kind === "room" && this.sel.id === id) this.sel = null;
  }

  /** Patch the selected room in place (no checkpoint — used by form typing + drag). */
  updateRoom(patch: Partial<MapRoom>): void {
    const id = this.selectedRoom?.id;
    if (id === undefined) return;
    this.rooms = this.rooms.map((r) => (r.id === id ? { ...r, ...patch } : r));
    this.dirty = true;
  }

  moveSelectedRoom(dx: number, dy: number): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    this.checkpoint();
    this.updateRoom({ x: sel.x + dx, y: sel.y + dy });
  }

  resizeSelectedRoom(edge: "left" | "right" | "top" | "bottom", grow: boolean): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    this.checkpoint();
    const d = grow ? 1 : -1;
    if (edge === "right") this.updateRoom({ w: clampInt(sel.w + d, 1) });
    else if (edge === "bottom") this.updateRoom({ h: clampInt(sel.h + d, 1) });
    else if (edge === "left") {
      const w = clampInt(sel.w + d, 1);
      this.updateRoom({ x: sel.x - (w - sel.w), w });
    } else {
      const h = clampInt(sel.h + d, 1);
      this.updateRoom({ y: sel.y - (h - sel.h), h });
    }
  }

  setRoomField(field: "name" | "x" | "y" | "w" | "h", value: string | number): void {
    if (field === "name") {
      this.updateRoom({ name: String(value) });
    } else {
      const min = field === "w" || field === "h" ? 1 : -1000;
      this.updateRoom({ [field]: clampInt(Number(value), min) } as Partial<MapRoom>);
    }
  }

  assignCamera(ip: string, dx: number, dy: number): boolean {
    const sel = this.selectedRoom;
    if (!sel || !ip) return false;
    if (sel.cameras.some((c) => c.ip === ip)) return false;
    this.checkpoint();
    const cx = Math.max(0, Math.min(dx, sel.w - 1));
    const cy = Math.max(0, Math.min(dy, sel.h - 1));
    this.updateRoom({ cameras: [...sel.cameras, { ip, dx: cx, dy: cy, rot: 0 }] });
    return true;
  }

  setCameraRotation(ip: string, rot: number): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    this.updateRoom({ cameras: sel.cameras.map((c) => (c.ip === ip ? { ...c, rot } : c)) });
  }

  /** Move an assigned camera within its room (offset from the room's top-left), clamped. */
  setCameraPosition(ip: string, dx: number, dy: number): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    const cx = Math.max(0, Math.min(Number.isFinite(dx) ? dx : 0, sel.w));
    const cy = Math.max(0, Math.min(Number.isFinite(dy) ? dy : 0, sel.h));
    this.updateRoom({ cameras: sel.cameras.map((c) => (c.ip === ip ? { ...c, dx: cx, dy: cy } : c)) });
  }

  /** Set a camera's FOV width (degrees) or view length (grid units). */
  setCameraFov(ip: string, fov: number): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    const v = Math.max(10, Math.min(Number.isFinite(fov) ? fov : 62, 180));
    this.updateRoom({ cameras: sel.cameras.map((c) => (c.ip === ip ? { ...c, fov: v } : c)) });
  }
  setCameraRange(ip: string, range: number): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    const v = Math.max(0.5, Math.min(Number.isFinite(range) ? range : 4.2, 30));
    this.updateRoom({ cameras: sel.cameras.map((c) => (c.ip === ip ? { ...c, range: v } : c)) });
  }

  /** Rotate the current selection by `deltaDeg` (room rot / marker rot / wall endpoints). */
  rotateSelected(deltaDeg: number): void {
    if (this.sel?.kind === "room") {
      const r = this.selectedRoom;
      if (!r) return;
      this.checkpoint();
      this.updateRoom({ rot: (((r.rot ?? 0) + deltaDeg) % 360 + 360) % 360 });
    } else if (this.sel?.kind === "marker") {
      const m = this.selectedMarker;
      if (!m) return;
      this.checkpoint();
      this.updateMarker(m.id, { rot: ((m.rot + deltaDeg) % 360 + 360) % 360 });
    } else if (this.sel?.kind === "wall") {
      const w = this.selectedWall;
      if (!w) return;
      this.checkpoint();
      const mx = (w.x1 + w.x2) / 2;
      const mz = (w.y1 + w.y2) / 2;
      const rad = (deltaDeg * Math.PI) / 180;
      const c = Math.cos(rad);
      const s = Math.sin(rad);
      const rot = (px: number, pz: number) => ({
        x: mx + (px - mx) * c - (pz - mz) * s,
        z: mz + (px - mx) * s + (pz - mz) * c,
      });
      const a = rot(w.x1, w.y1);
      const b = rot(w.x2, w.y2);
      this.updateWall(w.id, { x1: a.x, y1: a.z, x2: b.x, y2: b.z });
    } else if (this.sel?.kind === "text") {
      const t = this.selectedText;
      if (!t) return;
      this.checkpoint();
      this.updateText(t.id, { rot: (((t.rot ?? 0) + deltaDeg) % 360 + 360) % 360 });
    } else if (this.sel?.kind === "shape") {
      const s = this.selectedShape;
      if (!s) return;
      this.checkpoint();
      this.updateShape(s.id, { rot: (((s.rot ?? 0) + deltaDeg) % 360 + 360) % 360 });
    }
  }

  unassignCamera(ip: string): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    this.checkpoint();
    this.updateRoom({ cameras: sel.cameras.filter((c) => c.ip !== ip) });
  }

  // ---- walls --------------------------------------------------------------
  addWall(x1: number, y1: number, x2: number, y2: number, thickness = 0.12, style = "solid"): MapWall {
    this.checkpoint();
    const id = this.nextId(this.walls);
    const wall: MapWall = { id, x1, y1, x2, y2, thickness, style: style as MapWall["style"] };
    this.walls = [...this.walls, wall];
    this.sel = { kind: "wall", id };
    return wall;
  }

  /** Patch a wall in place (no checkpoint — used by drag + thickness slider). */
  updateWall(id: number, patch: Partial<MapWall>): void {
    this.walls = this.walls.map((w) => (w.id === id ? { ...w, ...patch } : w));
    this.dirty = true;
  }

  deleteWall(id: number): void {
    this.checkpoint();
    this.walls = this.walls.filter((w) => w.id !== id);
    if (this.sel?.kind === "wall" && this.sel.id === id) this.sel = null;
  }

  // ---- markers ------------------------------------------------------------
  addMarker(icon: string, x: number, y: number): MapMarker {
    this.checkpoint();
    const id = this.nextId(this.markers);
    const marker: MapMarker = { id, icon, x, y, rot: 0, scale: 1, label: null };
    this.markers = [...this.markers, marker];
    this.sel = { kind: "marker", id };
    return marker;
  }

  updateMarker(id: number, patch: Partial<MapMarker>): void {
    this.markers = this.markers.map((m) => (m.id === id ? { ...m, ...patch } : m));
    this.dirty = true;
  }

  deleteMarker(id: number): void {
    this.checkpoint();
    this.markers = this.markers.filter((m) => m.id !== id);
    if (this.sel?.kind === "marker" && this.sel.id === id) this.sel = null;
  }

  // ---- texts --------------------------------------------------------------
  addText(x: number, y: number, text = "Text"): MapText {
    this.checkpoint();
    const id = this.nextId(this.texts);
    const t: MapText = { id, text, x, y, size: 0.5, color: "#ffffff" };
    this.texts = [...this.texts, t];
    this.sel = { kind: "text", id };
    return t;
  }

  updateText(id: number, patch: Partial<MapText>): void {
    this.texts = this.texts.map((t) => (t.id === id ? { ...t, ...patch } : t));
    this.dirty = true;
  }

  deleteText(id: number): void {
    this.checkpoint();
    this.texts = this.texts.filter((t) => t.id !== id);
    if (this.sel?.kind === "text" && this.sel.id === id) this.sel = null;
  }

  // ---- shapes -------------------------------------------------------------
  addShape(shape: Omit<MapShape, "id">): MapShape {
    this.checkpoint();
    const id = this.nextId(this.shapes);
    const s: MapShape = { id, ...shape };
    this.shapes = [...this.shapes, s];
    this.sel = { kind: "shape", id };
    return s;
  }

  updateShape(id: number, patch: Partial<MapShape>): void {
    this.shapes = this.shapes.map((s) => (s.id === id ? { ...s, ...patch } : s));
    this.dirty = true;
  }

  deleteShape(id: number): void {
    this.checkpoint();
    this.shapes = this.shapes.filter((s) => s.id !== id);
    if (this.sel?.kind === "shape" && this.sel.id === id) this.sel = null;
  }

  /** Delete whatever is currently selected. */
  deleteSelected(): void {
    if (this.sel?.kind === "wall") this.deleteWall(this.sel.id);
    else if (this.sel?.kind === "marker") this.deleteMarker(this.sel.id);
    else if (this.sel?.kind === "text") this.deleteText(this.sel.id);
    else if (this.sel?.kind === "shape") this.deleteShape(this.sel.id);
  }

  /** Nudge the current selection by whole grid cells (keyboard arrows). */
  nudgeSelected(dx: number, dy: number): void {
    if (this.sel?.kind === "room") {
      this.moveSelectedRoom(dx, dy);
    } else if (this.sel?.kind === "wall") {
      const w = this.selectedWall;
      if (!w) return;
      this.checkpoint();
      this.updateWall(w.id, { x1: w.x1 + dx, y1: w.y1 + dy, x2: w.x2 + dx, y2: w.y2 + dy });
    } else if (this.sel?.kind === "marker") {
      const m = this.selectedMarker;
      if (!m) return;
      this.checkpoint();
      this.updateMarker(m.id, { x: m.x + dx, y: m.y + dy });
    } else if (this.sel?.kind === "text") {
      const t = this.selectedText;
      if (!t) return;
      this.checkpoint();
      this.updateText(t.id, { x: t.x + dx, y: t.y + dy });
    } else if (this.sel?.kind === "shape") {
      const s = this.selectedShape;
      if (!s) return;
      this.checkpoint();
      this.updateShape(s.id, { x: s.x + dx, y: s.y + dy });
    }
  }

  /** Cycle selection across all entities; dir ±1. */
  cycleSelection(dir: 1 | -1): void {
    const items: Selection[] = [
      ...this.rooms.map((r) => ({ kind: "room" as const, id: r.id })),
      ...this.walls.map((w) => ({ kind: "wall" as const, id: w.id })),
      ...this.markers.map((m) => ({ kind: "marker" as const, id: m.id })),
      ...this.texts.map((t) => ({ kind: "text" as const, id: t.id })),
      ...this.shapes.map((s) => ({ kind: "shape" as const, id: s.id })),
    ];
    if (items.length === 0) return;
    const cur = items.findIndex((i) => i!.kind === this.sel?.kind && i!.id === this.sel?.id);
    const next = (cur + dir + items.length) % items.length;
    this.sel = items[next];
  }
}
