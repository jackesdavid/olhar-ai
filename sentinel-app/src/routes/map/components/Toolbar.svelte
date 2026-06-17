<script lang="ts">
  // Map header: title + Save. (Drawing tools + undo/redo land in Phase 2.)
  import Save from "@lucide/svelte/icons/save";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { Button } from "$lib/components/ui/button";
  import type { MapStore } from "$lib/map/store.svelte";

  let { store, onSave }: { store: MapStore; onSave: () => void } = $props();
</script>

<div class="flex items-center justify-between">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Map</h1>
    <p class="text-sm text-muted-foreground">
      Top-down floor plan — arrange rooms, place cameras, and jump to Live.
    </p>
  </div>
  <Button onclick={onSave} disabled={store.saving || !store.dirty}>
    {#if store.saving}
      <LoaderCircle class="size-4 animate-spin" />
      Saving…
    {:else}
      <Save class="size-4" />
      Save{store.dirty ? " *" : ""}
    {/if}
  </Button>
</div>
