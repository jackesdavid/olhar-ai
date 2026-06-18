<script lang="ts">
  // One wall: a box from (x1,y1) to (x2,y2). Pivot is at the start endpoint so walls
  // can be corner-aligned. Two independent properties drive the look:
  //   • thickness → footprint depth.
  //   • style → "solid" raises an opaque shadow-casting wall; "wireframe" lays a flat
  //     footprint outline (edges only). Emerald is used only when selected.
  import { T } from "@threlte/core";
  import { BoxGeometry, EdgesGeometry } from "three";
  import type { MapWall } from "$lib/types";
  import { INK, INK_FILL, ACCENT_BRIGHT, ACCENT_SOFT, ACCENT_LABEL } from "$lib/map/palette";

  let { wall, selected = false }: { wall: MapWall; selected?: boolean } = $props();

  const dx = $derived(wall.x2 - wall.x1);
  const dz = $derived(wall.y2 - wall.y1);
  const len = $derived(Math.hypot(dx, dz));
  const angle = $derived(Math.atan2(dz, dx));

  const solid = $derived((wall.style ?? "solid") === "solid");
  const height = $derived(solid ? 0.8 : 0.04);
  const depth = $derived(Math.max(wall.thickness, 0.03));

  // Extend solid walls by depth/2 at each end so corners fill seamlessly at 90°.
  // Wireframe stays exact (it's a drawn line, extensions look odd).
  const geoLen = $derived(solid ? len + depth : len);
  const geo = $derived(new BoxGeometry(geoLen, height, depth));
  const edges = $derived(new EdgesGeometry(geo));

  const fillColor = $derived(selected ? ACCENT_SOFT : INK_FILL);
  const edgeColor = $derived(selected ? ACCENT_BRIGHT : INK);
</script>

{#if len > 0.001}
  <!-- Pivot at the wall start endpoint; mesh offset by len/2 so the box extends forward -->
  <T.Group position={[wall.x1, 0, wall.y1]} rotation.y={-angle}>
    {#if solid}
      <!-- Solid: opaque, no edge overlay -->
      <T.Mesh position={[len / 2, height / 2, 0]} geometry={geo} castShadow receiveShadow>
        <T.MeshStandardMaterial color={fillColor} roughness={0.65} metalness={0.06} />
      </T.Mesh>
    {:else}
      <!-- Wireframe: flat footprint outline only -->
      <T.LineSegments position={[len / 2, height / 2, 0]} geometry={edges}>
        <T.LineBasicMaterial color={edgeColor} />
      </T.LineSegments>
    {/if}

    {#if selected}
      {#each [0, len] as localX}
        <T.Mesh position={[localX, height + 0.12, 0]}>
          <T.SphereGeometry args={[0.14, 12, 12]} />
          <T.MeshBasicMaterial color={ACCENT_LABEL} />
        </T.Mesh>
      {/each}
    {/if}
  </T.Group>
{/if}
