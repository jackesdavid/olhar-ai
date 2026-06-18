<script lang="ts">
  // The dot-field floor: a cool blue-slate plane (the "paper" the plan is drawn on;
  // catches shadows from the wall extrusions), a tiled cool dot texture, and a soft
  // radial glow that makes the dots read brighter toward the focal centre.
  import { onMount } from "svelte";
  import { T } from "@threlte/core";
  import { DoubleSide, type Texture } from "three";
  import { dotFieldTexture, radialGlowTexture } from "./textures";
  import { FLOOR_BASE } from "$lib/map/palette";

  // World extent of the floor. Big enough to feel infinite at normal zoom; the
  // dot tile repeats once per grid unit so it always lines up with the grid.
  const SIZE = 400;

  let dots = $state<Texture | null>(null);
  let glow = $state<Texture | null>(null);

  onMount(() => {
    const d = dotFieldTexture();
    d.repeat.set(SIZE, SIZE); // one dot per world (grid) unit
    dots = d;
    glow = radialGlowTexture();
    return () => {
      d.dispose();
      glow?.dispose();
    };
  });
</script>

<!-- base floor: cool blue-slate — the paper the plan sits on -->
<T.Mesh rotation.x={-Math.PI / 2} position.y={0} receiveShadow>
  <T.PlaneGeometry args={[SIZE, SIZE]} />
  <T.MeshStandardMaterial color={FLOOR_BASE} roughness={0.95} metalness={0} />
</T.Mesh>

<!-- cool dot field -->
{#if dots}
  <T.Mesh rotation.x={-Math.PI / 2} position.y={0.02}>
    <T.PlaneGeometry args={[SIZE, SIZE]} />
    <T.MeshBasicMaterial map={dots} transparent opacity={0.4} depthWrite={false} side={DoubleSide} />
  </T.Mesh>
{/if}

<!-- focal radial glow (dot-field falloff) -->
{#if glow}
  <T.Mesh rotation.x={-Math.PI / 2} position.y={0.03}>
    <T.PlaneGeometry args={[SIZE * 0.5, SIZE * 0.5]} />
    <T.MeshBasicMaterial map={glow} transparent depthWrite={false} />
  </T.Mesh>
{/if}
