<script lang="ts">
  // One wall: a box from (x1,y1) to (x2,y2) with a crisp bright emerald edge outline
  // over a dark fill — the architectural top-down look from the references. Thickness
  // drives both width and height, so a thin value reads as a flat drawn line and a
  // thick one as a raised solid that catches the key light + casts a soft shadow.
  import { T } from "@threlte/core";
  import { BoxGeometry, EdgesGeometry } from "three";
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
  const depth = $derived(Math.max(wall.thickness, 0.03));

  const geo = $derived(new BoxGeometry(len, height, depth));
  const edges = $derived(new EdgesGeometry(geo));

  const fillColor = $derived(selected ? "#1f6b57" : "#22332e");
  const edgeColor = $derived(selected ? "#6ee7b7" : "#34d399");
  const edgeOpacity = $derived(selected ? 1 : 0.7);
</script>

{#if len > 0.001}
  <T.Mesh position={[midX, height / 2, midZ]} rotation.y={-angle} geometry={geo} castShadow receiveShadow>
    <T.MeshStandardMaterial color={fillColor} roughness={0.8} />
  </T.Mesh>
  <T.LineSegments position={[midX, height / 2, midZ]} rotation.y={-angle} geometry={edges}>
    <T.LineBasicMaterial color={edgeColor} transparent opacity={edgeOpacity} depthWrite={false} />
  </T.LineSegments>

  {#if selected}
    {#each [[wall.x1, wall.y1], [wall.x2, wall.y2]] as [hx, hz] (hx + ":" + hz)}
      <T.Mesh position={[hx, height + 0.12, hz]}>
        <T.SphereGeometry args={[0.14, 12, 12]} />
        <T.MeshBasicMaterial color="#d1fae5" />
      </T.Mesh>
    {/each}
  {/if}
{/if}
