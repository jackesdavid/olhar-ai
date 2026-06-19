<script lang="ts">
  // One wall: solid (extruded box), wireframe (box outline), dashed, or dotted.
  // Pivot is at the start endpoint for corner alignment. Color is overridable per wall.
  import { T } from "@threlte/core";
  import { BoxGeometry, EdgesGeometry, BufferGeometry, Vector3 } from "three";
  import type { MapWall } from "$lib/types";
  import { INK, INK_FILL, ACCENT_BRIGHT, ACCENT_SOFT, ACCENT_LABEL } from "$lib/map/palette";
  import LineStroke from "./LineStroke.svelte";

  let { wall, selected = false }: { wall: MapWall; selected?: boolean } = $props();

  const dx = $derived(wall.x2 - wall.x1);
  const dz = $derived(wall.y2 - wall.y1);
  const len = $derived(Math.hypot(dx, dz));
  const angle = $derived(Math.atan2(dz, dx));

  const style = $derived(wall.style ?? "solid");
  const solid = $derived(style === "solid");
  const height = $derived(solid ? 0.8 : 0.04);
  const depth = $derived(Math.max(wall.thickness, 0.03));

  // Extend box geometry by depth/2 at each end so corners fill seamlessly at 90°.
  // Applies to solid and wireframe; dashed/dotted/line use a plain centerline.
  const geoLen = $derived(style === "dashed" || style === "dotted" || style === "line" ? len : len + depth);
  const geo = $derived(new BoxGeometry(geoLen, height, depth));
  const edges = $derived(new EdgesGeometry(geo));

  // Line/dashed/dotted: a single centerline geometry (distances computed on the mesh).
  const lineGeo = $derived(
    new BufferGeometry().setFromPoints([new Vector3(0, 0, 0), new Vector3(len, 0, 0)])
  );
  const dashSize = $derived(style === "dotted" ? 0.04 : 0.25);
  const gapSize = $derived(style === "dotted" ? 0.14 : 0.14);

  // Color: wall.color override → selection emerald → palette default.
  const baseColor = $derived(wall.color ?? INK_FILL);
  const fillColor = $derived(selected ? ACCENT_SOFT : baseColor);
  const strokeColor = $derived(selected ? ACCENT_BRIGHT : (wall.color ?? INK));
</script>

{#if len > 0.001}
  <T.Group position={[wall.x1, 0, wall.y1]} rotation.y={-angle}>
    {#if solid}
      <T.Mesh position={[len / 2, height / 2, 0]} geometry={geo} castShadow receiveShadow>
        <T.MeshStandardMaterial color={fillColor} roughness={0.8} metalness={0} />
      </T.Mesh>

    {:else if style === "wireframe"}
      <T.LineSegments position={[len / 2, height / 2, 0]} geometry={edges}>
        <T.LineBasicMaterial color={strokeColor} />
      </T.LineSegments>

    {:else if style === "line"}
      <T.Group position={[0, 0.06, 0]}>
        <LineStroke geometry={lineGeo} color={strokeColor} />
      </T.Group>
    {:else}
      <!-- dashed or dotted centerline -->
      <T.Group position={[0, 0.06, 0]}>
        <LineStroke geometry={lineGeo} color={strokeColor} dashed dashSize={dashSize} gapSize={gapSize} />
      </T.Group>
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
