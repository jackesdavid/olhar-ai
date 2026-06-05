<script lang="ts">
  // Phase-0c — Bishop's floor-plan rooms as 3D objects. Threlte (declarative Three.js)
  // proves the map-in-3D path: real geometry, OrbitControls, click-to-select (raycasting),
  // all native JS — no WASM↔JS boundary. GLTF "real models" would slot in via `useGltf`.
  import { T } from "@threlte/core";
  import { OrbitControls, interactivity } from "@threlte/extras";

  type RoomCamera = { ip: string; dx: number; dy: number };
  type Room = {
    id: number;
    name: string;
    x: number;
    y: number;
    w: number;
    h: number;
    cameras: RoomCamera[];
  };

  let { rooms, selected = $bindable() }: { rooms: Room[]; selected: string | null } =
    $props();

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
  {@const isSel = selected === room.name}
  <T.Mesh
    position={[cx, WALL / 2, cz]}
    onclick={(e) => {
      e.stopPropagation();
      selected = room.name;
    }}
    onpointerenter={() => (document.body.style.cursor = "pointer")}
    onpointerleave={() => (document.body.style.cursor = "default")}
  >
    <T.BoxGeometry args={[room.w, WALL, room.h]} />
    <T.MeshStandardMaterial
      color={isSel ? "#f59e0b" : "#3b82f6"}
      transparent
      opacity={0.85}
      roughness={0.6}
    />
  </T.Mesh>

  <!-- camera markers (cyan cones) -->
  {#each room.cameras as cam (cam.ip)}
    <T.Mesh position={[room.x + cam.dx, WALL + 1.1, room.y + cam.dy]}>
      <T.ConeGeometry args={[0.6, 1.4, 12]} />
      <T.MeshStandardMaterial
        color="#22d3ee"
        emissive="#0891b2"
        emissiveIntensity={0.4}
      />
    </T.Mesh>
  {/each}
{/each}
