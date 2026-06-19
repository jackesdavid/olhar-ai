<script lang="ts">
  // Thin wrapper around T.Line that calls computeLineDistances() on the mesh
  // after mount — required for LineDashedMaterial to render dashes correctly.
  // computeLineDistances() lives on Line (the mesh), NOT on BufferGeometry.
  import { T } from "@threlte/core";
  import type { BufferGeometry } from "three";

  let {
    geometry,
    color,
    dashed = false,
    dashSize = 0.25,
    gapSize = 0.14,
  }: {
    geometry: BufferGeometry;
    color: string;
    dashed?: boolean;
    dashSize?: number;
    gapSize?: number;
  } = $props();

  let ref = $state<{ computeLineDistances(): void } | undefined>(undefined);

  $effect(() => {
    if (ref) ref.computeLineDistances();
  });
</script>

<T.Line {geometry} bind:ref>
  {#if dashed}
    <T.LineDashedMaterial {color} {dashSize} {gapSize} />
  {:else}
    <T.LineBasicMaterial {color} />
  {/if}
</T.Line>
