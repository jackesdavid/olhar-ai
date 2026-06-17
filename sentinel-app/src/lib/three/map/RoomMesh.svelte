<script lang="ts">
  // One room as a labeled ZONE — a faint floor fill + a thin boundary outline + a flat
  // label + its cameras. The whole room rotates about its centre via a group (room.rot),
  // so children are positioned relative to the centre. Walls are NOT implied — real walls
  // are drawn separately. Grid (x,y,w,h) → world XZ 1:1.
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import { DoubleSide } from "three";
  import type { MapRoom } from "$lib/types";
  import CameraMarker from "./CameraMarker.svelte";

  let {
    room,
    selected = false,
    onCameraClick,
  }: {
    room: MapRoom;
    selected?: boolean;
    onCameraClick?: (ip: string) => void;
  } = $props();

  // Boundary stroke: a thin flat bar lying on the floor (a drawn line, not a wall).
  const STROKE = 0.07;
  const STROKE_Y = 0.05;

  const cx = $derived(room.x + room.w / 2);
  const cz = $derived(room.y + room.h / 2);
  const rad = $derived(((room.rot ?? 0) * Math.PI) / 180);
  const labelSize = $derived(Math.max(0.5, Math.min(room.w, room.h) * 0.22));

  const strokeColor = $derived(selected ? "#34d399" : "#2f5147");
  const strokeOpacity = $derived(selected ? 0.95 : 0.4);
</script>

<!-- everything sits in a group centred on the room, rotated by room.rot -->
<T.Group position={[cx, 0, cz]} rotation.y={rad}>
  <!-- floor fill -->
  <T.Mesh rotation.x={-Math.PI / 2} position={[0, 0.04, 0]} receiveShadow>
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
    <T.Mesh position={[px, STROKE_Y, pz]}>
      <T.BoxGeometry args={[sx, 0.03, sz]} />
      <T.MeshBasicMaterial color={strokeColor} transparent opacity={strokeOpacity} />
    </T.Mesh>
  {/snippet}

  {@render stroke(0, -room.h / 2, room.w + STROKE, STROKE)}
  {@render stroke(0, room.h / 2, room.w + STROKE, STROKE)}
  {@render stroke(-room.w / 2, 0, STROKE, room.h)}
  {@render stroke(room.w / 2, 0, STROKE, room.h)}

  <!-- flat floor label -->
  <Text
    text={room.name}
    position={[0, 0.08, 0]}
    rotation={[-Math.PI / 2, 0, 0]}
    fontSize={labelSize}
    color={selected ? "#d1fae5" : "#7c9a8f"}
    anchorX="center"
    anchorY="middle"
  />

  <!-- cameras (positioned relative to the room centre) -->
  {#each room.cameras as cam (cam.ip)}
    <CameraMarker
      x={cam.dx - room.w / 2}
      z={cam.dy - room.h / 2}
      rot={cam.rot ?? 0}
      fov={cam.fov}
      range={cam.range}
      onClick={() => onCameraClick?.(cam.ip)}
    />
  {/each}
</T.Group>
