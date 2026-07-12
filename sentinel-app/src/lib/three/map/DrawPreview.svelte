<script lang="ts">
  // Rubber-band preview while drawing any shape. Shows a translucent guide from
  // the pending start point to the snapped cursor, in the map's drawing ink.
  import { T } from "@threlte/core";
  import { BufferGeometry, Vector3 } from "three";
  import { mapInk } from "$lib/map/palette.svelte";
  import type { ToolMode, Pt } from "$lib/map/tools.svelte";

  // Preview ink tracks the theme — these were hardcoded near-whites, which meant the
  // guide was invisible on Clean White. `wallEdge` is the map's drawing ink in both modes.
  const guide = $derived(mapInk.wallEdge);

  let {
    mode = "wall",
    from = null,
    to = null,
    draftB = null,
  }: {
    mode?: ToolMode;
    from?: Pt | null;
    to?: Pt | null;
    draftB?: Pt | null;
  } = $props();

  // Wall / line: segment from→to
  const seg = $derived.by(() => {
    if ((mode !== "wall" && mode !== "line") || !from || !to) return null;
    const dx = to.x - from.x;
    const dz = to.z - from.z;
    const len = Math.hypot(dx, dz);
    if (len < 0.001) return null;
    return { len, angle: Math.atan2(dz, dx), midX: (from.x + to.x) / 2, midZ: (from.z + to.z) / 2 };
  });

  // Rect: show outline from first corner to cursor
  const rectPreview = $derived.by(() => {
    if (mode !== "rect" || !from || !to) return null;
    const w = Math.abs(to.x - from.x);
    const h = Math.abs(to.z - from.z);
    if (w < 0.01 && h < 0.01) return null;
    return { cx: (from.x + to.x) / 2, cz: (from.z + to.z) / 2, w, h };
  });

  const rectGeo = $derived.by(() => {
    if (!rectPreview) return null;
    const { w, h } = rectPreview;
    const hw = w / 2, hh = h / 2;
    const pts = [
      new Vector3(-hw, 0, -hh), new Vector3(hw, 0, -hh),
      new Vector3(hw, 0, hh), new Vector3(-hw, 0, hh),
      new Vector3(-hw, 0, -hh),
    ];
    return new BufferGeometry().setFromPoints(pts);
  });

  // Circle: show circle outline at `from` with radius = dist(from, to)
  const circlePreview = $derived.by(() => {
    if (mode !== "circle" || !from || !to) return null;
    const r = Math.hypot(to.x - from.x, to.z - from.z);
    if (r < 0.01) return null;
    return { cx: from.x, cz: from.z, r };
  });

  const circleGeo = $derived.by(() => {
    if (!circlePreview) return null;
    const segs = 48;
    const pts = Array.from({ length: segs + 1 }, (_, i) => {
      const a = (i / segs) * Math.PI * 2;
      return new Vector3(Math.cos(a) * circlePreview.r, 0, Math.sin(a) * circlePreview.r);
    });
    return new BufferGeometry().setFromPoints(pts);
  });

  // Triangle: line from v0→v1 (phase 1) or v0→v1→cursor (phase 2)
  const triGeo = $derived.by(() => {
    if (mode !== "triangle" || !from) return null;
    const pts: Vector3[] = [new Vector3(from.x, 0, from.z)];
    if (draftB) pts.push(new Vector3(draftB.x, 0, draftB.z));
    if (to) pts.push(new Vector3(to.x, 0, to.z));
    if (draftB) pts.push(new Vector3(from.x, 0, from.z)); // close if we have 3 pts
    if (pts.length < 2) return null;
    return new BufferGeometry().setFromPoints(pts);
  });
</script>

<!-- Wall / line preview -->
{#if seg}
  <T.Mesh position={[seg.midX, 0.2, seg.midZ]} rotation.y={-seg.angle}>
    <T.BoxGeometry args={[seg.len, 0.06, 0.14]} />
    <T.MeshBasicMaterial color={guide} transparent opacity={0.55} depthWrite={false} />
  </T.Mesh>
{/if}

{#if (mode === "wall" || mode === "line") && from}
  <T.Mesh position={[from.x, 0.22, from.z]}>
    <T.SphereGeometry args={[0.16, 12, 12]} />
    <T.MeshBasicMaterial color={guide} />
  </T.Mesh>
{/if}

{#if (mode === "wall" || mode === "line") && to}
  <T.Mesh position={[to.x, 0.22, to.z]}>
    <T.SphereGeometry args={[0.13, 12, 12]} />
    <T.MeshBasicMaterial color={guide} transparent opacity={0.9} depthWrite={false} />
  </T.Mesh>
{/if}

<!-- Rect preview -->
{#if rectPreview && rectGeo}
  <T.Group position={[rectPreview.cx, 0.1, rectPreview.cz]}>
    <T.Line geometry={rectGeo}>
      <T.LineBasicMaterial color={guide} transparent opacity={0.7} />
    </T.Line>
  </T.Group>
  <T.Mesh position={[from!.x, 0.22, from!.z]}>
    <T.SphereGeometry args={[0.14, 12, 12]} />
    <T.MeshBasicMaterial color={guide} />
  </T.Mesh>
{/if}

<!-- Circle preview -->
{#if circlePreview && circleGeo}
  <T.Group position={[circlePreview.cx, 0.1, circlePreview.cz]}>
    <T.Line geometry={circleGeo}>
      <T.LineBasicMaterial color={guide} transparent opacity={0.7} />
    </T.Line>
  </T.Group>
  <T.Mesh position={[circlePreview.cx, 0.22, circlePreview.cz]}>
    <T.SphereGeometry args={[0.14, 12, 12]} />
    <T.MeshBasicMaterial color={guide} />
  </T.Mesh>
{/if}

<!-- Triangle preview -->
{#if triGeo}
  <T.Group position.y={0.1}>
    <T.Line geometry={triGeo}>
      <T.LineBasicMaterial color={guide} transparent opacity={0.7} />
    </T.Line>
  </T.Group>
  {#if from}
    <T.Mesh position={[from.x, 0.22, from.z]}>
      <T.SphereGeometry args={[0.14, 12, 12]} />
      <T.MeshBasicMaterial color={guide} />
    </T.Mesh>
  {/if}
  {#if draftB}
    <T.Mesh position={[draftB.x, 0.22, draftB.z]}>
      <T.SphereGeometry args={[0.13, 12, 12]} />
      <T.MeshBasicMaterial color={guide} />
    </T.Mesh>
  {/if}
{/if}

<!-- Marker / text: just a cursor dot at the hover point -->
{#if (mode === "marker" || mode === "text") && to}
  <T.Mesh position={[to.x, 0.22, to.z]}>
    <T.SphereGeometry args={[0.13, 12, 12]} />
    <T.MeshBasicMaterial color={guide} transparent opacity={0.9} depthWrite={false} />
  </T.Mesh>
{/if}
