<script lang="ts">
  // All walls in the plan. Selection highlight is driven by the store.
  import type { MapStore } from "$lib/map/store.svelte";
  import WallSegment from "./WallSegment.svelte";

  let { store }: { store: MapStore } = $props();
</script>

{#each store.walls as wall (wall.id)}
  <WallSegment {wall} selected={store.selectedWallId === wall.id} />
{/each}
