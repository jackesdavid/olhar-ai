<script lang="ts">
  // A camera on the plan: a small emerald body glyph plus its field-of-view rendered as a
  // wall-occluded line-of-sight polygon (it stops at walls, like light). Coordinates are
  // WORLD XZ (this renders in a world-space layer, not inside a room group). Clicking the
  // body jumps to Live.
  import { T } from "@threlte/core";
  import { DoubleSide, BufferGeometry, Float32BufferAttribute } from "three";
  import type { MapWall } from "$lib/types";
  import { fovPolygon } from "$lib/map/visibility";
  import { mapInk } from "$lib/map/palette.svelte";

  // Body + FOV ink track the theme — these were a hardcoded near-white, which put a
  // white camera on the white floor of Clean White.
  const bodyInk = $derived(mapInk.wallFill);
  const edgeInk = $derived(mapInk.wallEdge);

  let {
    x,
    z,
    rot = 0,
    fov = 62,
    range = 4.2,
    walls = [],
    onClick,
  }: {
    x: number;
    z: number;
    rot?: number; // world facing, degrees (0 = +z)
    fov?: number;
    range?: number;
    walls?: MapWall[];
    onClick?: () => void;
  } = $props();

  // Build the FOV polygon as a flat triangle-fan on the floor, occluded by walls.
  const fovGeo = $derived.by(() => {
    const facing = (rot * Math.PI) / 180;
    const cone = (fov * Math.PI) / 180;
    const steps = Math.max(48, Math.min(Math.round(fov), 180));
    const ring = fovPolygon(x, z, facing, cone, range, walls, steps);
    const verts: number[] = [x, 0.06, z]; // apex (camera)
    for (const p of ring) verts.push(p.x, 0.06, p.z);
    const idx: number[] = [];
    for (let i = 1; i < ring.length; i++) idx.push(0, i, i + 1);
    const g = new BufferGeometry();
    g.setAttribute("position", new Float32BufferAttribute(verts, 3));
    g.setIndex(idx);
    return g;
  });

  function pointer(on: boolean) {
    document.body.style.cursor = on ? "pointer" : "default";
  }
</script>

<!-- field-of-view: a flat polygon clipped by walls (line of sight) -->
<T.Mesh geometry={fovGeo}>
  <T.MeshBasicMaterial color={bodyInk} transparent opacity={0.1} depthWrite={false} side={DoubleSide} />
</T.Mesh>

<!-- camera body glyph -->
<T.Mesh
  position={[x, 0.32, z]}
  castShadow
  onclick={(e: { stopPropagation: () => void }) => {
    e.stopPropagation();
    onClick?.();
  }}
  onpointerenter={() => pointer(true)}
  onpointerleave={() => pointer(false)}
>
  <T.CylinderGeometry args={[0.32, 0.32, 0.36, 18]} />
  <T.MeshStandardMaterial color={bodyInk} emissive={edgeInk} emissiveIntensity={0.35} roughness={0.5} />
</T.Mesh>
