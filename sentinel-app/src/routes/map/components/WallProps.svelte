<script lang="ts">
  // Properties for the selected wall — two independent controls: style (filled solid
  // vs flat wireframe outline) and thickness (the wall's width), plus rotate + delete.
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { WallStyle } from "$lib/types";

  let { store }: { store: MapStore } = $props();

  const wall = $derived(store.selectedWall);
  const length = $derived(wall ? Math.hypot(wall.x2 - wall.x1, wall.y2 - wall.y1) : 0);

  const STYLE_LABELS: Record<WallStyle, string> = { solid: "Solid", wireframe: "Wireframe" };
  const style = $derived((wall?.style ?? "solid") as WallStyle);

  function widthLabel(t: number): string {
    if (t >= 0.3) return "Thick";
    if (t >= 0.15) return "Medium";
    return "Thin";
  }

  function setStyle(value: string) {
    if (!wall) return;
    store.checkpoint();
    store.updateWall(wall.id, { style: value as WallStyle });
  }
</script>

{#if wall}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <Label>Wall</Label>
      <span class="text-xs text-muted-foreground tabular-nums">{length.toFixed(1)} units</span>
    </div>

    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Style</Label>
      <Select.Root type="single" value={style} onValueChange={setStyle}>
        <Select.Trigger class="w-full">{STYLE_LABELS[style]}</Select.Trigger>
        <Select.Content>
          <Select.Item value="solid" label="Solid">Solid</Select.Item>
          <Select.Item value="wireframe" label="Wireframe">Wireframe</Select.Item>
        </Select.Content>
      </Select.Root>
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <Label class="text-xs text-muted-foreground">Thickness</Label>
        <span class="text-xs tabular-nums text-muted-foreground">{widthLabel(wall.thickness)}</span>
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
