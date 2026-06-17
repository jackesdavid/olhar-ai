<script lang="ts">
  // Selectable list of rooms with a camera-count badge.
  import CameraIcon from "@lucide/svelte/icons/camera";
  import { Badge } from "$lib/components/ui/badge";
  import type { MapStore } from "$lib/map/store.svelte";

  let { store }: { store: MapStore } = $props();

  function rowClass(id: number): string {
    const base =
      "flex items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors";
    const on = "border-primary/60 bg-primary/10";
    const off = "border-border hover:bg-muted/50";
    return `${base} ${store.selectedId === id ? on : off}`;
  }
</script>

<div class="flex flex-col gap-1">
  {#each store.rooms as room (room.id)}
    <button type="button" class={rowClass(room.id)} onclick={() => store.select(room.id)}>
      <span class="truncate">{room.name}</span>
      {#if room.cameras.length > 0}
        <Badge variant="secondary" class="ml-2 shrink-0">
          <CameraIcon class="mr-1 size-3" />
          {room.cameras.length}
        </Badge>
      {/if}
    </button>
  {/each}
</div>
