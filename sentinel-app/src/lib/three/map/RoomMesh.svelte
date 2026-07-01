<script lang="ts">
  // One room as a labeled ZONE — a faint floor fill + a thin boundary outline + a flat
  // label + its cameras. The whole room rotates about its centre via a group (room.rot),
  // so children are positioned relative to the centre. Walls are NOT implied — real walls
  // are drawn separately. Grid (x,y,w,h) → world XZ 1:1.
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import { DoubleSide } from "three";
  import type { MapRoom } from "$lib/types";
  import { ACCENT, ACCENT_SOFT, ACCENT_LABEL } from "$lib/map/palette";
  import { mapInk } from "$lib/map/palette.svelte";

  let {
    room,
    selected = false,
  }: {
    room: MapRoom;
    selected?: boolean;
  } = $props();

  // Boundary stroke: a thin flat bar lying on the floor (a drawn line, not a wall).
  const STROKE = 0.07;
  const STROKE_Y = 0.05;

  const cx = $derived(room.x + room.w / 2);
  const cz = $derived(room.y + room.h / 2);
  const rad = $derived(((room.rot ?? 0) * Math.PI) / 180);
  const labelSize = $derived(Math.max(0.5, Math.min(room.w, room.h) * 0.22));

  // Uppercase room labels read like a printed plan ("COCINA", "DORMITORIO").
  const labelText = $derived(room.name.toUpperCase());

  const fillColor = $derived(selected ? ACCENT_SOFT : mapInk.slateFill);
  const strokeColor = $derived(selected ? ACCENT : mapInk.slateLine);
  const strokeOpacity = $derived(selected ? 0.6 : 0.35);
  const labelColor = $derived(selected ? ACCENT_LABEL : mapInk.label);
</script>

<!-- everything sits in a group centred on the room, rotated by room.rot -->
<T.Group position={[cx, 0, cz]} rotation.y={rad}>
  <!-- floor fill -->
  <T.Mesh rotation.x={-Math.PI / 2} position={[0, 0.04, 0]} receiveShadow>
    <T.PlaneGeometry args={[room.w, room.h]} />
    <T.MeshStandardMaterial
      color={fillColor}
      transparent
      opacity={selected ? 0.16 : 0.32}
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

  <!-- flat floor label (uppercase, like a printed plan) -->
  <Text
    text={labelText}
    position={[0, 0.08, 0]}
    rotation={[-Math.PI / 2, 0, 0]}
    fontSize={labelSize}
    color={labelColor}
    anchorX="center"
    anchorY="middle"
  />

</T.Group>
