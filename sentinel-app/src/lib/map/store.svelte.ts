// MapStore — the editor's working copy of the floor plan: room zones, free-form
// walls, furniture/landmark markers, plus the device list, selection, undo/redo,
// and the dirty/saving flags. Pulled out of the page so the route shell stays thin.
//
// Mutations re-assign the relevant array (so Svelte 5 reactivity fires) and set
// `dirty`. Discrete actions checkpoint history first; continuous gestures (drag,
// sliders) checkpoint once at the start via `beginGesture()`. The whole plan saves
// as one { rooms, walls, markers } document.
import { data } from "$lib/data";
import type { Device, MapDoc, MapMarker, MapRoom, MapWall } from "$lib/types";
import { History } from "./history.svelte";

const DEFAULT_ROOM_W = 6;
const DEFAULT_ROOM_H = 4;

function clampInt(v: number, min: number): number {
  return Number.isFinite(v) ? Math.max(min, Math.round(v)) : min;
}

export type SelKind = "room" | "wall" | "marker";
export type Selection = { kind: SelKind; id: number } | null;

export class MapStore {
  rooms = $state<MapRoom[]>([]);
  walls = $state<MapWall[]>([]);
  markers = $state<MapMarker[]>([]);
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
  selectedRoomId = $derived(this.sel?.kind === "room" ? this.sel.id : null);
  selectedWallId = $derived(this.sel?.kind === "wall" ? this.sel.id : null);
  selectedMarkerId = $derived(this.sel?.kind === "marker" ? this.sel.id : null);

  selectRoom(id: number | null): void {
    this.sel = id === null ? null : { kind: "room", id };
  }
  selectWall(id: number): void {
    this.sel = { kind: "wall", id };
  }
  selectMarker(id: number): void {
    this.sel = { kind: "marker", id };
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
    return { rooms: this.rooms, walls: this.walls, markers: this.markers };
  }
  private apply(doc: MapDoc): void {
    this.rooms = doc.rooms;
    this.walls = doc.walls;
    this.markers = doc.markers;
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

  unassignCamera(ip: string): void {
    const sel = this.selectedRoom;
    if (!sel) return;
    this.checkpoint();
    this.updateRoom({ cameras: sel.cameras.filter((c) => c.ip !== ip) });
  }

  // ---- walls --------------------------------------------------------------
  addWall(x1: number, y1: number, x2: number, y2: number, thickness = 0.12): MapWall {
    this.checkpoint();
    const id = this.nextId(this.walls);
    const wall: MapWall = { id, x1, y1, x2, y2, thickness };
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

  /** Delete whatever is currently selected. */
  deleteSelected(): void {
    if (this.sel?.kind === "wall") this.deleteWall(this.sel.id);
    else if (this.sel?.kind === "marker") this.deleteMarker(this.sel.id);
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
    }
  }

  /** Cycle selection across all entities (rooms → walls → markers); dir ±1. */
  cycleSelection(dir: 1 | -1): void {
    const items: Selection[] = [
      ...this.rooms.map((r) => ({ kind: "room" as const, id: r.id })),
      ...this.walls.map((w) => ({ kind: "wall" as const, id: w.id })),
      ...this.markers.map((m) => ({ kind: "marker" as const, id: m.id })),
    ];
    if (items.length === 0) return;
    const cur = items.findIndex((i) => i!.kind === this.sel?.kind && i!.id === this.sel?.id);
    const next = (cur + dir + items.length) % items.length;
    this.sel = items[next];
  }
}
