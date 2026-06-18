<script lang="ts">
  // One wall: a box from (x1,y1) to (x2,y2) with a crisp near-white edge outline — the
  // architectural top-down look from the references. Two independent properties drive it:
  //   • thickness → the wall's WIDTH (footprint depth).
  //   • style → "solid" raises a filled, shadow-casting wall; "wireframe" lays a flat
  //     footprint outline (edges only) like a drawn line. Emerald is used only when the
  //     wall is selected; the resting plan stays neutral.
  import { T } from "@threlte/core";
  import { BoxGeometry, EdgesGeometry } from "three";
  import type { MapWall } from "$lib/types";
  import { INK, INK_FILL, ACCENT_BRIGHT, ACCENT_SOFT, ACCENT_LABEL } from "$lib/map/palette";

  let { wall, selected = false }: { wall: MapWall; selected?: boolean } = $props();

  const dx = $derived(wall.x2 - wall.x1);
  const dz = $derived(wall.y2 - wall.y1);
  const len = $derived(Math.hypot(dx, dz));
  const angle = $derived(Math.atan2(dz, dx));
  const midX = $derived((wall.x1 + wall.x2) / 2);
  const midZ = $derived((wall.y1 + wall.y2) / 2);

  const solid = $derived((wall.style ?? "solid") === "solid");
  // solid → raised wall that catches the key light; wireframe → flat footprint outline.
  const height = $derived(solid ? 0.8 : 0.04);
  const depth = $derived(Math.max(wall.thickness, 0.03));

  const geo = $derived(new BoxGeometry(len, height, depth));
  const edges = $derived(new EdgesGeometry(geo));

  const fillColor = $derived(selected ? ACCENT_SOFT : INK_FILL);
  const edgeColor = $derived(selected ? ACCENT_BRIGHT : INK);
  const edgeOpacity = $derived(selected ? 1 : 0.85);
</script>

{#if len > 0.001}
  <!-- filled body: solid walls only (wireframe is the outline alone) -->
  {#if solid}
    <T.Mesh position={[midX, height / 2, midZ]} rotation.y={-angle} geometry={geo} castShadow receiveShadow>
      <T.MeshStandardMaterial color={fillColor} roughness={0.65} metalness={0.06} />
    </T.Mesh>
  {/if}
  <T.LineSegments position={[midX, height / 2, midZ]} rotation.y={-angle} geometry={edges}>
    <T.LineBasicMaterial color={edgeColor} transparent opacity={edgeOpacity} depthWrite={false} />
  </T.LineSegments>

  {#if selected}
    {#each [[wall.x1, wall.y1], [wall.x2, wall.y2]] as [hx, hz] (hx + ":" + hz)}
      <T.Mesh position={[hx, height + 0.12, hz]}>
        <T.SphereGeometry args={[0.14, 12, 12]} />
        <T.MeshBasicMaterial color={ACCENT_LABEL} />
      </T.Mesh>
    {/each}
  {/if}
{/if}
