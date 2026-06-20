<script lang="ts">
  // All furniture / landmark markers. Selection highlight driven by the store.
  import type { MapStore } from "$lib/map/store.svelte";
  import MarkerGlyph from "./MarkerGlyph.svelte";

  let { store }: { store: MapStore } = $props();
</script>

{#each store.markers as marker (marker.id)}
  <MarkerGlyph {marker} selected={store.selectedMarkerId === marker.id} />
{/each}
