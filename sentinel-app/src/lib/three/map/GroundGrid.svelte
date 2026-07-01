<script lang="ts">
  // The dot-field floor: a cool blue-slate plane (the "paper" the plan is drawn on;
  // catches shadows from the wall extrusions), a tiled cool dot texture, and a soft
  // radial glow that makes the dots read brighter toward the focal centre.
  import { T } from "@threlte/core";
  import { DoubleSide, type Texture } from "three";
  import { dotGridTexture, radialGlowTexture } from "./textures";
  import { mapInk } from "$lib/map/palette.svelte";

  // World extent of the floor. Big enough to feel infinite at normal zoom; the
  // dot tile repeats once per grid unit so it always lines up with the grid.
  const SIZE = 400;

  let dots = $state<Texture | null>(null);
  let glow = $state<Texture | null>(null);

  // Rebuild the floor textures whenever the theme (ink) changes, so the dot-field
  // and glow flip with light/dark. Threlte updates the floor colour reactively.
  $effect(() => {
    const d = dotGridTexture(mapInk.gridMinor, mapInk.gridMajor);
    d.repeat.set(SIZE / 5, SIZE / 5); // tile covers 5 world units (major division)
    const g = radialGlowTexture(mapInk.glow);
    dots = d;
    glow = g;
    return () => {
      d.dispose();
      g.dispose();
    };
  });
</script>

<!-- base floor: cool blue-slate — the paper the plan sits on -->
<T.Mesh rotation.x={-Math.PI / 2} position.y={0} receiveShadow>
  <T.PlaneGeometry args={[SIZE, SIZE]} />
  <T.MeshStandardMaterial color={mapInk.floor} roughness={0.95} metalness={0} />
</T.Mesh>

<!-- box grid -->
{#if dots}
  <T.Mesh rotation.x={-Math.PI / 2} position.y={0.02}>
    <T.PlaneGeometry args={[SIZE, SIZE]} />
    <T.MeshBasicMaterial map={dots} transparent opacity={1} depthWrite={false} side={DoubleSide} />
  </T.Mesh>
{/if}

<!-- focal radial glow (dot-field falloff) -->
{#if glow}
  <T.Mesh rotation.x={-Math.PI / 2} position.y={0.03}>
    <T.PlaneGeometry args={[SIZE * 0.5, SIZE * 0.5]} />
    <T.MeshBasicMaterial map={glow} transparent depthWrite={false} />
  </T.Mesh>
{/if}
