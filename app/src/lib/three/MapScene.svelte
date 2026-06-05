<script lang="ts">
  // Unit 018 — Bishop's floor-plan rooms as 3D objects. Threlte (declarative
  // Three.js): real geometry, OrbitControls, click-to-select (raycasting), all
  // native JS — no WASM↔JS boundary. Adapted from web-spike-0a/Scene.svelte.
  import { T } from "@threlte/core";
  import { OrbitControls, interactivity } from "@threlte/extras";

  import type { MapRoom } from "$lib/types";

  let {
    rooms,
    selected = $bindable(),
    onCameraClick,
  }: {
    rooms: MapRoom[];
    selected: number | null;
    onCameraClick?: (ip: string) => void;
  } = $props();

  interactivity(); // enables pointer events (raycasting) on meshes

  const WALL = 2.5; // extruded room height

  // Orbit target = centroid of all rooms, so any map frames itself.
  const center = $derived.by(() => {
    if (!rooms.length) return { x: 17, z: 5 };
    let sx = 0,
      sz = 0;
    for (const r of rooms) {
      sx += r.x + r.w / 2;
      sz += r.y + r.h / 2;
    }
    return { x: sx / rooms.length, z: sz / rooms.length };
  });
</script>

<T.PerspectiveCamera makeDefault position={[center.x, 30, center.z + 36]} fov={50}>
  <OrbitControls enableDamping target.x={center.x} target.z={center.z} />
</T.PerspectiveCamera>

<T.AmbientLight intensity={0.7} />
<T.DirectionalLight position={[center.x + 24, 44, center.z + 14]} intensity={1.3} />

<!-- ground -->
<T.Mesh rotation.x={-Math.PI / 2} position={[center.x, 0, center.z]}>
  <T.PlaneGeometry args={[300, 300]} />
  <T.MeshStandardMaterial color="#0e1320" />
</T.Mesh>

{#each rooms as room (room.id)}
  {@const cx = room.x + room.w / 2}
  {@const cz = room.y + room.h / 2}
  {@const isSel = selected === room.id}
  <T.Mesh
    position={[cx, WALL / 2, cz]}
    onclick={(e) => {
      e.stopPropagation();
      selected = room.id;
    }}
    onpointerenter={() => (document.body.style.cursor = "pointer")}
    onpointerleave={() => (document.body.style.cursor = "default")}
  >
    <T.BoxGeometry args={[Math.max(room.w, 0.1), WALL, Math.max(room.h, 0.1)]} />
    <T.MeshStandardMaterial
      color={isSel ? "#f59e0b" : "#3b82f6"}
      transparent
      opacity={0.85}
      roughness={0.6}
    />
  </T.Mesh>

  <!-- camera markers (cyan cones) — clicking one jumps to Live -->
  {#each room.cameras as cam (cam.ip)}
    <T.Mesh
      position={[room.x + cam.dx, WALL + 1.1, room.y + cam.dy]}
      onclick={(e) => {
        e.stopPropagation();
        onCameraClick?.(cam.ip);
      }}
      onpointerenter={() => (document.body.style.cursor = "pointer")}
      onpointerleave={() => (document.body.style.cursor = "default")}
    >
      <T.ConeGeometry args={[0.6, 1.4, 12]} />
      <T.MeshStandardMaterial color="#22d3ee" emissive="#0891b2" emissiveIntensity={0.4} />
    </T.Mesh>
  {/each}
{/each}
