<script lang="ts">
  // All cameras, rendered in WORLD space (not inside the rotated room groups) so their
  // wall-occluded FOV polygons can be computed against world walls. Each camera's world
  // pose is derived from its room: position = room centre + rotate(offset, room.rot);
  // facing = camera.rot + room.rot. Mirrors the math in presence.locate().
  import type { MapStore } from "$lib/map/store.svelte";
  import type { MapRoom, RoomCamera } from "$lib/types";
  import CameraMarker from "./CameraMarker.svelte";

  let { store, onCameraClick }: { store: MapStore; onCameraClick?: (ip: string) => void } = $props();

  function pose(room: MapRoom, cam: RoomCamera): { x: number; z: number; facing: number } {
    const cx0 = room.x + room.w / 2;
    const cz0 = room.y + room.h / 2;
    const rad = ((room.rot ?? 0) * Math.PI) / 180;
    const lx = cam.dx - room.w / 2;
    const lz = cam.dy - room.h / 2;
    const c = Math.cos(rad);
    const s = Math.sin(rad);
    return {
      x: cx0 + lx * c + lz * s,
      z: cz0 - lx * s + lz * c,
      facing: (cam.rot ?? 0) + (room.rot ?? 0),
    };
  }
</script>

{#each store.rooms as room (room.id)}
  {#each room.cameras as cam (cam.ip)}
    {@const p = pose(room, cam)}
    <CameraMarker
      x={p.x}
      z={p.z}
      rot={p.facing}
      fov={cam.fov}
      range={cam.range}
      walls={store.walls}
      onClick={() => onCameraClick?.(cam.ip)}
    />
  {/each}
{/each}
