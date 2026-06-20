<script lang="ts">
  // One furniture / landmark marker, drawn as a top-down architectural plan symbol:
  // each SVG shape becomes a thin near-white outline (line loop) over a faint fill —
  // the wireframe look from the references. Selected markers go emerald. Oriented by
  // yaw, sized by scale, pivoted on the icon's logical anchor. Cached per icon key.
  import { onMount } from "svelte";
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import { DoubleSide, ShapeGeometry, BufferGeometry, Vector3 } from "three";
  import { SVGLoader } from "three/examples/jsm/loaders/SVGLoader.js";
  import type { MapMarker } from "$lib/types";
  import { iconSvg, iconAnchor } from "$lib/map/icons";
  import { INK, LABEL, ACCENT_BRIGHT, ACCENT_LABEL } from "$lib/map/palette";

  let { marker, selected = false }: { marker: MapMarker; selected?: boolean } = $props();

  const BASE = 1.6; // world size of a scale-1 glyph (24 svg units → BASE)

  type Built = { fills: ShapeGeometry[]; lines: BufferGeometry[] };
  const cache = new Map<string, Built>();

  function build(key: string): Built {
    const hit = cache.get(key);
    if (hit) return hit;
    const [ax, ay] = iconAnchor(key);
    const fills: ShapeGeometry[] = [];
    const lines: BufferGeometry[] = [];
    // anchor → origin, with SVG y-down flipped to y-up.
    const toLoop = (pts: { x: number; y: number }[]): BufferGeometry =>
      new BufferGeometry().setFromPoints(pts.map((p) => new Vector3(p.x - ax, ay - p.y, 0)));
    try {
      const data = new SVGLoader().parse(iconSvg(key));
      for (const path of data.paths) {
        for (const shape of SVGLoader.createShapes(path)) {
          const g = new ShapeGeometry(shape);
          g.translate(-ax, -ay, 0);
          g.scale(1, -1, 1);
          fills.push(g);
          const outline = shape.extractPoints(16);
          lines.push(toLoop(outline.shape));
          for (const hole of outline.holes) lines.push(toLoop(hole));
        }
      }
    } catch (err) {
      console.error("marker glyph parse failed", key, err);
    }
    const built = { fills, lines };
    cache.set(key, built);
    return built;
  }

  let built = $state<Built>({ fills: [], lines: [] });
  onMount(() => {
    built = build(marker.icon);
  });
  // rebuild when the icon changes
  $effect(() => {
    built = build(marker.icon);
  });

  const yaw = $derived((-marker.rot * Math.PI) / 180);
  const lineColor = $derived(selected ? ACCENT_BRIGHT : INK);
  const lineOpacity = $derived(selected ? 1 : 0.85);
  const fillOpacity = $derived(selected ? 0.18 : 0.1);
  const labelColor = $derived(selected ? ACCENT_LABEL : LABEL);
</script>

<T.Group position={[marker.x, 0.08, marker.y]} rotation.y={yaw} scale={marker.scale}>
  <T.Group rotation.x={-Math.PI / 2} scale={BASE / 24}>
    {#each built.fills as g, i (i)}
      <T.Mesh geometry={g}>
        <T.MeshBasicMaterial color={lineColor} transparent opacity={fillOpacity} side={DoubleSide} depthWrite={false} />
      </T.Mesh>
    {/each}
    {#each built.lines as g, i (i)}
      <T.LineLoop geometry={g}>
        <T.LineBasicMaterial color={lineColor} transparent opacity={lineOpacity} depthWrite={false} />
      </T.LineLoop>
    {/each}
  </T.Group>
</T.Group>

{#if marker.label}
  <Text
    text={marker.label}
    position={[marker.x, 0.09, marker.y + BASE * marker.scale * 0.62]}
    rotation={[-Math.PI / 2, 0, 0]}
    fontSize={0.42}
    color={labelColor}
    anchorX="center"
    anchorY="middle"
  />
{/if}
