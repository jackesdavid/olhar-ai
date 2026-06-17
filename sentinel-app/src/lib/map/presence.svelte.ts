// PresenceStore — live presence pulses on the map. Subscribes to the shared SSE
// stream's `presence` events (relayed from sentinel-flow's RecordPresence node),
// places each on the floor (at the detecting camera, else the room centre), and
// expires it after a few seconds so the map "breathes" then settles.
import { data, onEvent } from "$lib/data";
import type { PresenceCategory, PresenceEvent } from "$lib/types";
import type { MapStore } from "./store.svelte";

const DEFAULT_TTL_MS = 6000;
// On open, replay presence recorded in the last window as a fresh pulse — so a detection
// fired moments before you opened the map still shows (the live SSE only covers events
// that arrive while the page is mounted).
const REPLAY_WINDOW_MS = 30000;

export type Pulse = {
  id: number;
  category: PresenceCategory;
  x: number;
  z: number;
  born: number;
  ttl: number;
};

export class PresenceStore {
  pulses = $state<Pulse[]>([]);

  private map: MapStore;
  private seq = 0;
  private unsub: (() => void) | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(map: MapStore) {
    this.map = map;
  }

  async start(): Promise<void> {
    this.unsub = await onEvent("presence", (p) => this.add(p as PresenceEvent));
    this.timer = setInterval(() => this.prune(), 500);
    // Seed from recent history so presence fired just before the map opened still shows.
    // Wait for the map's rooms to load first — otherwise locate() can't resolve a
    // position and the replay is dropped.
    try {
      for (let i = 0; i < 100 && this.map.loading; i++) {
        await new Promise((r) => setTimeout(r, 50));
      }
      const recent = await data.loadPresence(50);
      const now = Date.now();
      for (const r of recent) {
        const age = now - Date.parse(r.created_at);
        if (Number.isFinite(age) && age >= 0 && age <= REPLAY_WINDOW_MS) this.add(r);
      }
    } catch (err) {
      console.warn("[presence] baseline load failed", err);
    }
  }

  stop(): void {
    this.unsub?.();
    this.unsub = null;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  /** Where to draw the pulse: the detecting camera, else the room centre. */
  private locate(p: PresenceEvent): { x: number; z: number } | null {
    if (p.camera) {
      // p.camera may be a device id (uuid, how workflow nodes bind cameras) or an ip;
      // rooms store cameras by ip, so resolve through the device list first.
      const dev = this.map.devices.find((d) => d.id === p.camera || d.ip === p.camera);
      const ip = dev?.ip ?? p.camera;
      for (const r of this.map.rooms) {
        const c = r.cameras.find((cam) => cam.ip === ip);
        if (c) {
          // Camera world position: rotate its in-room offset about the room centre.
          const rrad = ((r.rot ?? 0) * Math.PI) / 180;
          const cwx = Math.cos(rrad);
          const swx = Math.sin(rrad);
          const lx = c.dx - r.w / 2;
          const lz = c.dy - r.h / 2;
          const camX = r.x + r.w / 2 + lx * cwx + lz * swx;
          const camZ = r.y + r.h / 2 - lx * swx + lz * cwx;
          // Drop the pulse ahead in the camera's field of view (facing = cam.rot + room.rot;
          // forward = (-sin, cos), matching CameraMarker), scaled to the camera's range.
          const a = (((c.rot ?? 0) + (r.rot ?? 0)) * Math.PI) / 180;
          const ahead = Math.max(1, (c.range ?? 4.2) * 0.55);
          return { x: camX - Math.sin(a) * ahead, z: camZ + Math.cos(a) * ahead };
        }
      }
    }
    if (p.room_id != null) {
      const r = this.map.rooms.find((x) => x.id === p.room_id);
      if (r) return { x: r.x + r.w / 2, z: r.y + r.h / 2 };
    }
    if (p.room_name) {
      const name = p.room_name.toLowerCase();
      const r = this.map.rooms.find((x) => x.name.toLowerCase() === name);
      if (r) return { x: r.x + r.w / 2, z: r.y + r.h / 2 };
    }
    return null;
  }

  private add(p: PresenceEvent): void {
    const at = this.locate(p);
    if (!at) {
      // Most common cause: the room/camera on the event doesn't match any on the map.
      console.warn("[presence] dropped — no map location for", p);
      return;
    }
    const ttl = p.duration_secs && p.duration_secs > 0 ? p.duration_secs * 1000 : DEFAULT_TTL_MS;
    console.debug("[presence] pulse", p.category, "at", at, "for", ttl / 1000, "s");
    this.pulses = [
      ...this.pulses,
      { id: this.seq++, category: p.category, x: at.x, z: at.z, born: Date.now(), ttl },
    ];
  }

  private prune(): void {
    const now = Date.now();
    const live = this.pulses.filter((x) => now - x.born < x.ttl);
    if (live.length !== this.pulses.length) this.pulses = live;
  }
}
