<script lang="ts">
  // One furniture / landmark marker: its SVG icon parsed into flat geometry and laid
  // on the floor as an emerald glyph, oriented by yaw and sized by scale. Built on
  // the client (SVGLoader uses DOMParser); geometries are cached per icon key.
  import { onMount } from "svelte";
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import { DoubleSide, ShapeGeometry } from "three";
  import { SVGLoader } from "three/examples/jsm/loaders/SVGLoader.js";
  import type { MapMarker } from "$lib/types";
  import { iconSvg } from "$lib/map/icons";

  let { marker, selected = false }: { marker: MapMarker; selected?: boolean } = $props();

  const BASE = 1.6; // world size of a scale-1 glyph (24 svg units → BASE)

  const cache = new Map<string, ShapeGeometry[]>();

  function build(key: string): ShapeGeometry[] {
    const hit = cache.get(key);
    if (hit) return hit;
    const out: ShapeGeometry[] = [];
    try {
      const data = new SVGLoader().parse(iconSvg(key));
      for (const path of data.paths) {
        for (const shape of SVGLoader.createShapes(path)) {
          const g = new ShapeGeometry(shape);
          g.translate(-12, -12, 0); // centre the 24×24 viewBox
          g.scale(1, -1, 1); // SVG y-down → y-up
          out.push(g);
        }
      }
    } catch (err) {
      console.error("marker glyph parse failed", key, err);
    }
    cache.set(key, out);
    return out;
  }

  let geoms = $state<ShapeGeometry[]>([]);
  onMount(() => {
    geoms = build(marker.icon);
  });
  // rebuild when the icon changes
  $effect(() => {
    geoms = build(marker.icon);
  });

  const yaw = $derived((-marker.rot * Math.PI) / 180);
  const color = $derived(selected ? "#34d399" : "#5b8f80");
</script>

<T.Group position={[marker.x, 0.08, marker.y]} rotation.y={yaw} scale={marker.scale}>
  <T.Group rotation.x={-Math.PI / 2} scale={BASE / 24}>
    {#each geoms as g, i (i)}
      <T.Mesh geometry={g}>
        <T.MeshBasicMaterial {color} transparent opacity={selected ? 0.95 : 0.82} side={DoubleSide} depthWrite={false} />
      </T.Mesh>
    {/each}
  </T.Group>
</T.Group>

{#if marker.label}
  <Text
    text={marker.label}
    position={[marker.x, 0.09, marker.y + BASE * marker.scale * 0.62]}
    rotation={[-Math.PI / 2, 0, 0]}
    fontSize={0.42}
    color={selected ? "#d1fae5" : "#7c9a8f"}
    anchorX="center"
    anchorY="middle"
  />
{/if}
