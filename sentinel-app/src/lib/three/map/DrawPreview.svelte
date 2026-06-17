<script lang="ts">
  // Rubber-band preview while drawing a wall: a translucent emerald segment from
  // the pending start point to the snapped cursor, plus dots at both ends. Shown
  // only by the Wall tool.
  import { T } from "@threlte/core";
  import type { Pt } from "$lib/map/tools.svelte";

  let { from = null, to = null }: { from?: Pt | null; to?: Pt | null } = $props();

  const seg = $derived.by(() => {
    if (!from || !to) return null;
    const dx = to.x - from.x;
    const dz = to.z - from.z;
    const len = Math.hypot(dx, dz);
    if (len < 0.001) return null;
    return {
      len,
      angle: Math.atan2(dz, dx),
      midX: (from.x + to.x) / 2,
      midZ: (from.z + to.z) / 2,
    };
  });
</script>

{#if from && seg}
  <T.Mesh position={[seg.midX, 0.2, seg.midZ]} rotation.y={-seg.angle}>
    <T.BoxGeometry args={[seg.len, 0.06, 0.14]} />
    <T.MeshBasicMaterial color="#34d399" transparent opacity={0.55} depthWrite={false} />
  </T.Mesh>
{/if}

{#if from}
  <T.Mesh position={[from.x, 0.22, from.z]}>
    <T.SphereGeometry args={[0.16, 12, 12]} />
    <T.MeshBasicMaterial color="#34d399" />
  </T.Mesh>
{/if}

{#if to}
  <T.Mesh position={[to.x, 0.22, to.z]}>
    <T.SphereGeometry args={[0.13, 12, 12]} />
    <T.MeshBasicMaterial color="#a7f3d0" transparent opacity={0.9} depthWrite={false} />
  </T.Mesh>
{/if}
