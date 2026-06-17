<script lang="ts">
  // A camera on the plan: a small emerald glyph plus a translucent field-of-view
  // wedge fanning out along its yaw. Reads correctly from straight above. Clicking
  // it jumps to the Live view for that camera.
  import { T } from "@threlte/core";
  import { DoubleSide } from "three";

  let {
    x,
    z,
    rot = 0,
    onClick,
  }: {
    x: number;
    z: number;
    rot?: number;
    onClick?: () => void;
  } = $props();

  const FOV_DEG = 62;
  const RANGE = 4.2; // grid units
  const fov = (FOV_DEG * Math.PI) / 180;

  // rot is degrees, 0 = facing +y (down on the plan / world +z). Base offset of
  // -90° makes the wedge's local 0-angle point along +z; rot then turns it.
  const yaw = $derived(-Math.PI / 2 - (rot * Math.PI) / 180);

  function pointer(on: boolean) {
    document.body.style.cursor = on ? "pointer" : "default";
  }
</script>

<!-- field-of-view wedge: a flat circular sector on the floor -->
<T.Mesh position={[x, 0.06, z]} rotation={[-Math.PI / 2, 0, yaw]}>
  <T.CircleGeometry args={[RANGE, 28, -fov / 2, fov]} />
  <T.MeshBasicMaterial color="#34d399" transparent opacity={0.12} depthWrite={false} side={DoubleSide} />
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
  <T.MeshStandardMaterial color="#34d399" emissive="#10b981" emissiveIntensity={0.55} roughness={0.4} />
</T.Mesh>
