<script lang="ts">
  // One wall: a box from (x1,y1) to (x2,y2). Thickness drives both width and
  // height, so a thin value reads as a flat drawn line and a thick one as a raised
  // solid wall (which catches the key light for depth). Open top — never a ceiling.
  import { T } from "@threlte/core";
  import type { MapWall } from "$lib/types";

  let { wall, selected = false }: { wall: MapWall; selected?: boolean } = $props();

  const dx = $derived(wall.x2 - wall.x1);
  const dz = $derived(wall.y2 - wall.y1);
  const len = $derived(Math.hypot(dx, dz));
  const angle = $derived(Math.atan2(dz, dx));
  const midX = $derived((wall.x1 + wall.x2) / 2);
  const midZ = $derived((wall.y1 + wall.y2) / 2);
  // thin line → flat strip; thick → raised wall.
  const height = $derived(Math.max(0.05, wall.thickness * 2));

  const color = $derived(selected ? "#34d399" : "#3f6b5e");
  const emissive = $derived(selected ? "#10b981" : "#0a221c");
</script>

{#if len > 0.001}
  <T.Mesh position={[midX, height / 2, midZ]} rotation.y={-angle} castShadow receiveShadow>
    <T.BoxGeometry args={[len, height, Math.max(wall.thickness, 0.03)]} />
    <T.MeshStandardMaterial
      {color}
      {emissive}
      emissiveIntensity={selected ? 0.5 : 0.2}
      roughness={0.7}
    />
  </T.Mesh>

  {#if selected}
    {#each [[wall.x1, wall.y1], [wall.x2, wall.y2]] as [hx, hz] (hx + ":" + hz)}
      <T.Mesh position={[hx, height + 0.12, hz]}>
        <T.SphereGeometry args={[0.14, 12, 12]} />
        <T.MeshBasicMaterial color="#d1fae5" />
      </T.Mesh>
    {/each}
  {/if}
{/if}
