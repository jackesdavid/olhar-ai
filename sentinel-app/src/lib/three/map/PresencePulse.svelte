<script lang="ts">
  // The "ping": concentric rings radiating from a point (a sensor detecting), with a
  // bright centre dot. Self-animating; the PresenceStore controls how long it lives.
  import { T, useTask } from "@threlte/core";
  import { DoubleSide } from "three";

  let { x, z, color = "#34d399" }: { x: number; z: number; color?: string } = $props();

  const RINGS = [0, 1, 2];
  const PERIOD = 1.6; // seconds per ring cycle

  let t = $state(0);
  useTask((dt) => {
    t += dt;
  });

  function ring(i: number): { scale: number; opacity: number } {
    const phase = (t / PERIOD + i / RINGS.length) % 1;
    return { scale: 0.4 + phase * 2.4, opacity: (1 - phase) * 0.7 };
  }
</script>

<!-- centre dot -->
<T.Mesh position={[x, 0.3, z]}>
  <T.SphereGeometry args={[0.22, 16, 16]} />
  <T.MeshBasicMaterial {color} />
</T.Mesh>

<!-- radiating rings -->
{#each RINGS as i (i)}
  {@const r = ring(i)}
  <T.Mesh position={[x, 0.14, z]} rotation.x={-Math.PI / 2} scale={r.scale}>
    <T.RingGeometry args={[0.5, 0.64, 36]} />
    <T.MeshBasicMaterial {color} transparent opacity={r.opacity} side={DoubleSide} depthWrite={false} />
  </T.Mesh>
{/each}
