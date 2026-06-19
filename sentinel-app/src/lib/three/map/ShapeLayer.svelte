<script lang="ts">
  // Renders MapShape annotations (rect / triangle / circle) flat on the floor.
  import { T } from "@threlte/core";
  import { BufferGeometry, BufferAttribute, Vector3 } from "three";
  import type { MapShape } from "$lib/types";
  import type { MapStore } from "$lib/map/store.svelte";
  import { ACCENT_SOFT, ACCENT_BRIGHT, INK } from "$lib/map/palette";
  import LineStroke from "./LineStroke.svelte";

  let { store }: { store: MapStore } = $props();

  function rectOutlineGeo(w: number, h: number): BufferGeometry {
    const hw = w / 2, hh = h / 2;
    const pts = [
      new Vector3(-hw, 0, -hh), new Vector3(hw, 0, -hh),
      new Vector3(hw, 0, hh), new Vector3(-hw, 0, hh),
      new Vector3(-hw, 0, -hh),
    ];
    return new BufferGeometry().setFromPoints(pts);
  }

  function circleOutlineGeo(r: number, segs = 64): BufferGeometry {
    const pts = Array.from({ length: segs + 1 }, (_, i) => {
      const a = (i / segs) * Math.PI * 2;
      return new Vector3(Math.cos(a) * r, 0, Math.sin(a) * r);
    });
    return new BufferGeometry().setFromPoints(pts);
  }

  function triOutlineGeo(pts: [number, number][]): BufferGeometry {
    const [a, b, c] = pts;
    const verts = [
      new Vector3(a[0], 0, a[1]), new Vector3(b[0], 0, b[1]),
      new Vector3(c[0], 0, c[1]), new Vector3(a[0], 0, a[1]),
    ];
    return new BufferGeometry().setFromPoints(verts);
  }

  function triFillGeo(pts: [number, number][]): BufferGeometry {
    const verts = new Float32Array(9);
    verts[0] = pts[0][0]; verts[1] = 0; verts[2] = pts[0][1];
    verts[3] = pts[1][0]; verts[4] = 0; verts[5] = pts[1][1];
    verts[6] = pts[2][0]; verts[7] = 0; verts[8] = pts[2][1];
    const g = new BufferGeometry();
    g.setAttribute("position", new BufferAttribute(verts, 3));
    return g;
  }

  function sc(s: MapShape, selected: boolean): string {
    return selected ? ACCENT_BRIGHT : (s.color ?? INK);
  }
  function isDashed(s: MapShape): boolean {
    return s.strokeStyle === "dashed" || s.strokeStyle === "dotted";
  }
  function ds(s: MapShape): number { return s.strokeStyle === "dotted" ? 0.04 : 0.25; }
  function rr(s: MapShape): number { return ((s.rot ?? 0) * Math.PI) / 180; }
</script>

{#each store.shapes as s (s.id)}
  {@const sel = s.id === store.selectedShapeId}
  {@const stroke = sc(s, sel)}
  {@const fillCol = sel ? ACCENT_SOFT : s.fill}

  {#if s.kind === "rect" && s.w != null && s.h != null}
    {@const outGeo = rectOutlineGeo(s.w, s.h)}
    <T.Group position={[s.x, 0, s.y]} rotation.y={-rr(s)}>
      {#if fillCol}
        <T.Mesh rotation.x={-Math.PI / 2} position.y={0.03}>
          <T.PlaneGeometry args={[s.w, s.h]} />
          <T.MeshBasicMaterial color={fillCol} transparent opacity={0.22} depthWrite={false} />
        </T.Mesh>
      {/if}
      <T.Group position.y={0.05}>
        <LineStroke geometry={outGeo} color={stroke} dashed={isDashed(s)} dashSize={ds(s)} gapSize={0.14} />
      </T.Group>
    </T.Group>

  {:else if s.kind === "circle" && s.r != null}
    {@const outGeo = circleOutlineGeo(s.r)}
    <T.Group position={[s.x, 0, s.y]}>
      {#if fillCol}
        <T.Mesh rotation.x={-Math.PI / 2} position.y={0.03}>
          <T.CircleGeometry args={[s.r, 64]} />
          <T.MeshBasicMaterial color={fillCol} transparent opacity={0.22} depthWrite={false} />
        </T.Mesh>
      {/if}
      <T.Group position.y={0.05}>
        <LineStroke geometry={outGeo} color={stroke} dashed={isDashed(s)} dashSize={ds(s)} gapSize={0.14} />
      </T.Group>
    </T.Group>

  {:else if s.kind === "triangle" && s.pts && s.pts.length === 3}
    {@const outGeo = triOutlineGeo(s.pts)}
    {@const fGeo = fillCol ? triFillGeo(s.pts) : null}
    <T.Group position.y={0}>
      {#if fillCol && fGeo}
        <T.Mesh geometry={fGeo} position.y={0.03}>
          <T.MeshBasicMaterial color={fillCol} transparent opacity={0.22} depthWrite={false} side={2} />
        </T.Mesh>
      {/if}
      <T.Group position.y={0.05}>
        <LineStroke geometry={outGeo} color={stroke} dashed={isDashed(s)} dashSize={ds(s)} gapSize={0.14} />
      </T.Group>
    </T.Group>
  {/if}
{/each}
