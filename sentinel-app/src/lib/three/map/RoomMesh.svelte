<script lang="ts">
  // One room as a labeled ZONE — not a walled box. A faint floor fill + a thin
  // boundary outline (brighter when selected) + a flat label + its cameras. Walls
  // are NOT implied: real walls are drawn separately (Phase 2), so open spaces like
  // a backyard or open garage simply have no walls. Grid (x,y,w,h) → world XZ 1:1.
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import { DoubleSide } from "three";
  import type { MapRoom } from "$lib/types";
  import CameraMarker from "./CameraMarker.svelte";

  let {
    room,
    selected = false,
    onSelect,
    onCameraClick,
  }: {
    room: MapRoom;
    selected?: boolean;
    onSelect?: () => void;
    onCameraClick?: (ip: string) => void;
  } = $props();

  // Boundary stroke: a thin flat bar lying on the floor (a drawn line, not a wall).
  const STROKE = 0.07;
  const STROKE_Y = 0.05;

  const cx = $derived(room.x + room.w / 2);
  const cz = $derived(room.y + room.h / 2);
  const labelSize = $derived(Math.max(0.5, Math.min(room.w, room.h) * 0.22));

  const strokeColor = $derived(selected ? "#34d399" : "#2f5147");
  const strokeOpacity = $derived(selected ? 0.95 : 0.4);

  function pointer(on: boolean) {
    document.body.style.cursor = on ? "pointer" : "default";
  }

  function pick(e: { stopPropagation: () => void }) {
    e.stopPropagation();
    onSelect?.();
  }
</script>

<!-- floor fill / click target -->
<T.Mesh
  rotation.x={-Math.PI / 2}
  position={[cx, 0.04, cz]}
  receiveShadow
  onclick={pick}
  onpointerenter={() => pointer(true)}
  onpointerleave={() => pointer(false)}
>
  <T.PlaneGeometry args={[room.w, room.h]} />
  <T.MeshStandardMaterial
    color={selected ? "#10b981" : "#1c2a26"}
    transparent
    opacity={selected ? 0.16 : 0.42}
    roughness={0.9}
    side={DoubleSide}
  />
</T.Mesh>

<!-- boundary outline (flat strokes on the floor — shows the zone, not walls) -->
{#snippet stroke(px: number, pz: number, sx: number, sz: number)}
  <T.Mesh position={[px, STROKE_Y, pz]} onclick={pick}>
    <T.BoxGeometry args={[sx, 0.03, sz]} />
    <T.MeshBasicMaterial color={strokeColor} transparent opacity={strokeOpacity} />
  </T.Mesh>
{/snippet}

{@render stroke(cx, room.y, room.w + STROKE, STROKE)}
{@render stroke(cx, room.y + room.h, room.w + STROKE, STROKE)}
{@render stroke(room.x, cz, STROKE, room.h)}
{@render stroke(room.x + room.w, cz, STROKE, room.h)}

<!-- flat floor label -->
<Text
  text={room.name}
  position={[cx, 0.08, cz]}
  rotation={[-Math.PI / 2, 0, 0]}
  fontSize={labelSize}
  color={selected ? "#d1fae5" : "#7c9a8f"}
  anchorX="center"
  anchorY="middle"
/>

<!-- cameras -->
{#each room.cameras as cam (cam.ip)}
  <CameraMarker
    x={room.x + cam.dx}
    z={room.y + cam.dy}
    rot={cam.rot ?? 0}
    onClick={() => onCameraClick?.(cam.ip)}
  />
{/each}
