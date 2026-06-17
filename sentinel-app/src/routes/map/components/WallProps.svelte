<script lang="ts">
  // Properties for the selected wall: thickness (line ↔ solid wall), rotation, delete.
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import type { MapStore } from "$lib/map/store.svelte";

  let { store }: { store: MapStore } = $props();

  const wall = $derived(store.selectedWall);
  const length = $derived(wall ? Math.hypot(wall.x2 - wall.x1, wall.y2 - wall.y1) : 0);

  function kindLabel(t: number): string {
    return t >= 0.22 ? "Solid wall" : "Line";
  }
</script>

{#if wall}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <Label>Wall</Label>
      <span class="text-xs text-muted-foreground tabular-nums">{length.toFixed(1)} units</span>
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <Label class="text-xs text-muted-foreground">Thickness</Label>
        <span class="text-xs tabular-nums text-muted-foreground">{kindLabel(wall.thickness)}</span>
      </div>
      <input
        type="range"
        min="0.05"
        max="0.6"
        step="0.01"
        value={wall.thickness}
        class="h-1.5 w-full accent-primary"
        onpointerdown={() => store.checkpoint()}
        oninput={(e) => store.updateWall(wall.id, { thickness: e.currentTarget.valueAsNumber })}
      />
    </div>

    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Rotate (about centre)</Label>
      <div class="flex items-center gap-2">
        <Button variant="outline" size="sm" class="flex-1" onclick={() => store.rotateSelected(-15)}>
          <RotateCcw class="size-4" /> −15°
        </Button>
        <Button variant="outline" size="sm" class="flex-1" onclick={() => store.rotateSelected(15)}>
          <RotateCw class="size-4" /> +15°
        </Button>
      </div>
    </div>

    <Button variant="destructive" size="sm" class="w-full" onclick={() => store.deleteWall(wall.id)}>
      <Trash2 class="size-4" />
      Delete wall
    </Button>
  </div>
{/if}
