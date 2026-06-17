// MapStore — the editor's working copy of the floor plan. Owns the rooms array,
// the device list (for camera assignment), selection, and the dirty/saving flags.
// Pulled out of the page so the route shell stays thin and the logic is testable.
//
// Mutations go through small methods that re-assign `rooms` (so Svelte 5 reactivity
// fires) and set `dirty`. The whole plan is saved as one document via `data.saveMap`.
import { data } from "$lib/data";
import type { Device, MapMarker, MapRoom, MapWall } from "$lib/types";

const DEFAULT_ROOM_W = 6;
const DEFAULT_ROOM_H = 4;

function clampInt(v: number, min: number): number {
  return Number.isFinite(v) ? Math.max(min, Math.round(v)) : min;
}

export class MapStore {
  rooms = $state<MapRoom[]>([]);
  walls = $state<MapWall[]>([]);
  markers = $state<MapMarker[]>([]);
  devices = $state<Device[]>([]);
  selectedId = $state<number | null>(null);
  loading = $state(true);
  saving = $state(false);
  dirty = $state(false);

  selected = $derived(this.rooms.find((r) => r.id === this.selectedId) ?? null);
  deviceByIp = $derived(new Map(this.devices.map((d) => [d.ip, d])));

  deviceName(ip: string): string {
    return this.deviceByIp.get(ip)?.name ?? ip;
  }

  /** Devices not yet assigned to the selected room — candidates for the picker. */
  get assignable(): Device[] {
    const sel = this.selected;
    if (!sel) return [];
    return this.devices.filter((d) => !sel.cameras.some((c) => c.ip === d.ip));
  }

  private nextRoomId(): number {
    return this.rooms.reduce((m, r) => Math.max(m, r.id), -1) + 1;
  }

  async load(): Promise<void> {
    this.loading = true;
    try {
      const [doc, ds] = await Promise.all([data.loadMap(), data.listDevices()]);
      this.rooms = doc.rooms ?? [];
      this.walls = doc.walls ?? [];
      this.markers = doc.markers ?? [];
      this.devices = ds;
      this.selectedId = this.rooms.length > 0 ? this.rooms[0].id : null;
      this.dirty = false;
    } finally {
      this.loading = false;
    }
  }

  select(id: number | null): void {
    this.selectedId = id;
  }

  addRoom(): MapRoom {
    const id = this.nextRoomId();
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
    this.selectedId = id;
    this.dirty = true;
    return room;
  }

  deleteRoom(id: number): void {
    this.rooms = this.rooms.filter((r) => r.id !== id);
    if (this.selectedId === id) {
      this.selectedId = this.rooms.length > 0 ? this.rooms[0].id : null;
    }
    this.dirty = true;
  }

  /** Patch the selected room in place (re-assigns rooms + flags dirty). */
  updateSelected(patch: Partial<MapRoom>): void {
    if (this.selectedId === null) return;
    this.rooms = this.rooms.map((r) => (r.id === this.selectedId ? { ...r, ...patch } : r));
    this.dirty = true;
  }

  /** Nudge the selected room by whole grid cells (keyboard move). */
  moveSelected(dx: number, dy: number): void {
    const sel = this.selected;
    if (!sel) return;
    this.updateSelected({ x: sel.x + dx, y: sel.y + dy });
  }

  /** Grow/shrink the selected room toward one edge by one cell (keyboard resize).
   * Growing left/top moves the origin so the opposite edge stays put. */
  resizeSelected(edge: "left" | "right" | "top" | "bottom", grow: boolean): void {
    const sel = this.selected;
    if (!sel) return;
    const d = grow ? 1 : -1;
    if (edge === "right") this.updateSelected({ w: clampInt(sel.w + d, 1) });
    else if (edge === "bottom") this.updateSelected({ h: clampInt(sel.h + d, 1) });
    else if (edge === "left") {
      const w = clampInt(sel.w + d, 1);
      this.updateSelected({ x: sel.x - (w - sel.w), w });
    } else {
      const h = clampInt(sel.h + d, 1);
      this.updateSelected({ y: sel.y - (h - sel.h), h });
    }
  }

  setRoomField(field: "name" | "x" | "y" | "w" | "h", value: string | number): void {
    if (field === "name") {
      this.updateSelected({ name: String(value) });
    } else {
      const min = field === "w" || field === "h" ? 1 : -1000;
      this.updateSelected({ [field]: clampInt(Number(value), min) } as Partial<MapRoom>);
    }
  }

  assignCamera(ip: string, dx: number, dy: number): boolean {
    const sel = this.selected;
    if (!sel || !ip) return false;
    if (sel.cameras.some((c) => c.ip === ip)) return false;
    const cx = Math.max(0, Math.min(dx, sel.w - 1));
    const cy = Math.max(0, Math.min(dy, sel.h - 1));
    this.updateSelected({ cameras: [...sel.cameras, { ip, dx: cx, dy: cy, rot: 0 }] });
    return true;
  }

  setCameraRotation(ip: string, rot: number): void {
    const sel = this.selected;
    if (!sel) return;
    this.updateSelected({
      cameras: sel.cameras.map((c) => (c.ip === ip ? { ...c, rot } : c)),
    });
  }

  unassignCamera(ip: string): void {
    const sel = this.selected;
    if (!sel) return;
    this.updateSelected({ cameras: sel.cameras.filter((c) => c.ip !== ip) });
  }

  async save(): Promise<void> {
    this.saving = true;
    try {
      await data.saveMap({ rooms: this.rooms, walls: this.walls, markers: this.markers });
      this.dirty = false;
    } finally {
      this.saving = false;
    }
  }
}
