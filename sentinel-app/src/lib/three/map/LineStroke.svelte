<script lang="ts">
  // Wrapper around T.Line that manages its material and dash distances imperatively.
  // Threlte's child T.LineDashedMaterial doesn't reliably propagate color/dash changes,
  // and computeLineDistances() lives on the Line mesh — not on BufferGeometry.
  // This component owns both: it recreates the material and re-runs computeLineDistances
  // whenever any prop changes.
  import { T } from "@threlte/core";
  import { LineBasicMaterial, LineDashedMaterial, type BufferGeometry, type Line } from "three";

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

  let ref = $state<Line | undefined>(undefined);

  $effect(() => {
    // Track all props so the effect re-runs whenever any one changes.
    // Threlte sets ref.geometry = geometry before effects fire, so computeLineDistances
    // will see the latest geometry.
    void geometry;
    if (!ref) return;
    ref.material = dashed
      ? new LineDashedMaterial({ color, dashSize, gapSize })
      : new LineBasicMaterial({ color });
    ref.computeLineDistances();
  });
</script>

<T.Line {geometry} bind:ref />
