// PresenceStore — live presence pulses on the map. Subscribes to the shared SSE
// stream's `presence` events (relayed from sentinel-flow's RecordPresence node),
// places each on the floor (at the detecting camera, else the room centre), and
// expires it after a few seconds so the map "breathes" then settles.
import { onEvent } from "$lib/data";
import type { PresenceCategory, PresenceEvent } from "$lib/types";
import type { MapStore } from "./store.svelte";

const TTL_MS = 6000;

export type Pulse = {
  id: number;
  category: PresenceCategory;
  x: number;
  z: number;
  born: number;
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
      for (const r of this.map.rooms) {
        const c = r.cameras.find((cam) => cam.ip === p.camera);
        if (c) return { x: r.x + c.dx, z: r.y + c.dy };
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
    if (!at) return;
    this.pulses = [
      ...this.pulses,
      { id: this.seq++, category: p.category, x: at.x, z: at.z, born: Date.now() },
    ];
  }

  private prune(): void {
    const now = Date.now();
    const live = this.pulses.filter((x) => now - x.born < TTL_MS);
    if (live.length !== this.pulses.length) this.pulses = live;
  }
}
