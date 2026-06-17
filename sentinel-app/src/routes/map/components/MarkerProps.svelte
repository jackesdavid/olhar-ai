<script lang="ts">
  // Properties for the selected marker: icon, rotation, scale, label, delete.
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { MARKER_ICONS } from "$lib/map/icons";
  import type { MapStore } from "$lib/map/store.svelte";

  let { store }: { store: MapStore } = $props();

  const marker = $derived(store.selectedMarker);

  function tileClass(key: string): string {
    const base =
      "flex aspect-square items-center justify-center rounded-md border p-1.5 text-primary transition-colors [&_svg]:size-6";
    const on = "border-primary/60 bg-primary/10";
    const off = "border-border hover:bg-muted/50";
    return `${base} ${marker?.icon === key ? on : off}`;
  }
</script>

{#if marker}
  <div class="space-y-3">
    <Label>Marker</Label>

    <div class="grid grid-cols-5 gap-1.5">
      {#each MARKER_ICONS as icon (icon.key)}
        <button
          type="button"
          class={tileClass(icon.key)}
          title={icon.label}
          onclick={() => store.updateMarker(marker.id, { icon: icon.key })}
        >
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          {@html icon.svg}
        </button>
      {/each}
    </div>

    <div class="space-y-1.5">
      <Label for="marker-label" class="text-xs text-muted-foreground">Label</Label>
      <Input
        id="marker-label"
        value={marker.label ?? ""}
        placeholder="Optional caption"
        oninput={(e) => store.updateMarker(marker.id, { label: e.currentTarget.value })}
      />
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <Label class="text-xs text-muted-foreground">Rotation</Label>
        <span class="text-xs tabular-nums text-muted-foreground">{Math.round(marker.rot)}°</span>
      </div>
      <input
        type="range"
        min="0"
        max="359"
        value={marker.rot}
        class="h-1.5 w-full accent-primary"
        onpointerdown={() => store.checkpoint()}
        oninput={(e) => store.updateMarker(marker.id, { rot: e.currentTarget.valueAsNumber })}
      />
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <Label class="text-xs text-muted-foreground">Size</Label>
        <span class="text-xs tabular-nums text-muted-foreground">{marker.scale.toFixed(1)}×</span>
      </div>
      <input
        type="range"
        min="0.5"
        max="3"
        step="0.1"
        value={marker.scale}
        class="h-1.5 w-full accent-primary"
        onpointerdown={() => store.checkpoint()}
        oninput={(e) => store.updateMarker(marker.id, { scale: e.currentTarget.valueAsNumber })}
      />
    </div>

    <Button variant="destructive" size="sm" class="w-full" onclick={() => store.deleteMarker(marker.id)}>
      <Trash2 class="size-4" />
      Delete marker
    </Button>
  </div>
{/if}
