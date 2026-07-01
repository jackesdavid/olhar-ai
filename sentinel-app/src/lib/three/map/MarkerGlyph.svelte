<script lang="ts">
  // One furniture / landmark marker, drawn as a top-down architectural plan symbol:
  // each SVG shape becomes a thin near-white outline (line loop) over a faint fill —
  // the wireframe look from the references. Selected markers go emerald.
  //
  // Supports two icon modes (see icons.ts):
  //   inline  — svg string, parsed synchronously on first use.
  //   src URL — fetched once, stripped of any white background rect, then parsed.
  //             The `vb` field normalises any viewBox to the same world footprint.
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import { DoubleSide, ShapeGeometry, BufferGeometry, Vector3 } from "three";
  import { SVGLoader } from "three/examples/jsm/loaders/SVGLoader.js";
  import type { MapMarker } from "$lib/types";
  import { iconSvg, iconSrc, iconAnchor, iconVb } from "$lib/map/icons";
  import { ACCENT_BRIGHT, ACCENT_LABEL } from "$lib/map/palette";
  import { mapInk } from "$lib/map/palette.svelte";

  let { marker, selected = false }: { marker: MapMarker; selected?: boolean } = $props();

  const BASE = 1.6; // world size of a scale-1 glyph (largest SVG dim → BASE world units)

  type Built = { fills: ShapeGeometry[]; lines: BufferGeometry[] };
  // Shared caches across all MarkerGlyph instances.
  const builtCache = new Map<string, Built>();
  const textCache = new Map<string, string>(); // key → fetched SVG text
  const inFlight = new Map<string, Promise<string>>(); // deduplicate concurrent fetches

  function buildFromText(key: string, svgText: string): Built {
    const [ax, ay] = iconAnchor(key);
    const fills: ShapeGeometry[] = [];
    const lines: BufferGeometry[] = [];
    const toLoop = (pts: { x: number; y: number }[]): BufferGeometry =>
      new BufferGeometry().setFromPoints(pts.map((p) => new Vector3(p.x - ax, ay - p.y, 0)));
    try {
      const data = new SVGLoader().parse(svgText);
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
    const result = { fills, lines };
    builtCache.set(key, result);
    return result;
  }

  function buildInline(key: string): Built {
    if (builtCache.has(key)) return builtCache.get(key)!;
    return buildFromText(key, iconSvg(key));
  }

  async function fetchSrc(key: string, src: string): Promise<string> {
    if (textCache.has(key)) return textCache.get(key)!;
    if (!inFlight.has(key)) {
      const p = fetch(src)
        .then((r) => r.text())
        .then((text) => {
          // Strip white background rects that vectorisers add (e.g. fill="#fefefe").
          const cleaned = text.replace(/<rect[^>]+fill="#fefefe"[^>]*\/?>/g, "");
          textCache.set(key, cleaned);
          return cleaned;
        });
      inFlight.set(key, p);
    }
    return inFlight.get(key)!;
  }

  let built = $state<Built>({ fills: [], lines: [] });

  // Rebuild geometry whenever the icon key changes.
  // For src icons: return empty immediately, update once fetch completes.
  $effect(() => {
    const key = marker.icon;
    if (builtCache.has(key)) {
      built = builtCache.get(key)!;
      return;
    }
    const src = iconSrc(key);
    if (src) {
      built = { fills: [], lines: [] };
      fetchSrc(key, src).then((text) => {
        builtCache.delete(key); // force rebuild
        built = buildFromText(key, text);
      });
    } else {
      built = buildInline(key);
    }
  });

  // vb: normalise any viewBox to BASE world units (inline icons default to 24).
  const vb = $derived(iconVb(marker.icon));
  const yaw = $derived((-marker.rot * Math.PI) / 180);
  const lineColor = $derived(selected ? ACCENT_BRIGHT : mapInk.wallEdge);
  const lineOpacity = $derived(selected ? 1 : 0.85);
  const fillOpacity = $derived(selected ? 0.18 : 0.1);
  const labelColor = $derived(selected ? ACCENT_LABEL : mapInk.label);
</script>

<T.Group position={[marker.x, 0.08, marker.y]} rotation.y={yaw} scale={marker.scale}>
  <T.Group rotation.x={-Math.PI / 2} scale={BASE / vb}>
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
