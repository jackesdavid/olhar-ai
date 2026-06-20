<script lang="ts">
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import RotateCcwSquare from "@lucide/svelte/icons/rotate-ccw";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { WallStyle } from "$lib/types";

  let { store }: { store: MapStore } = $props();

  const wall = $derived(store.selectedWall);
  const length = $derived(wall ? Math.hypot(wall.x2 - wall.x1, wall.y2 - wall.y1) : 0);

  const STYLES: { value: WallStyle; label: string }[] = [
    { value: "solid",     label: "Solid" },
    { value: "wireframe", label: "Wireframe" },
    { value: "line",      label: "Line" },
    { value: "dashed",    label: "Dashed" },
    { value: "dotted",    label: "Dotted" },
  ];

  const style = $derived((wall?.style ?? "solid") as WallStyle);
  const styleLabel = $derived(STYLES.find((s) => s.value === style)?.label ?? "Solid");

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

  function setColor(value: string) {
    if (!wall) return;
    store.updateWall(wall.id, { color: value });
  }

  function resetColor() {
    if (!wall) return;
    store.checkpoint();
    store.updateWall(wall.id, { color: undefined });
  }
</script>

{#if wall}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <Label>Wall</Label>
      <span class="text-xs text-muted-foreground tabular-nums">{length.toFixed(1)} units</span>
    </div>

    <!-- Style -->
    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Style</Label>
      <Select.Root type="single" value={style} onValueChange={setStyle}>
        <Select.Trigger class="w-full">{styleLabel}</Select.Trigger>
        <Select.Content>
          {#each STYLES as s (s.value)}
            <Select.Item value={s.value} label={s.label}>{s.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    <!-- Color -->
    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Color</Label>
      <div class="flex items-center gap-2">
        <input
          type="color"
          value={wall.color ?? "#ffffff"}
          class="h-8 w-10 cursor-pointer rounded border border-border bg-transparent p-0.5"
          onpointerdown={() => store.checkpoint()}
          oninput={(e) => setColor(e.currentTarget.value)}
        />
        {#if wall.color}
          <Button variant="ghost" size="sm" class="h-8 text-xs text-muted-foreground" onclick={resetColor}>
            Reset
          </Button>
        {:else}
          <span class="text-xs text-muted-foreground">Default</span>
        {/if}
      </div>
    </div>

    <!-- Thickness (not shown for flat styles — they're always 1px) -->
    {#if style === "solid" || style === "wireframe"}
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
    {/if}

    <!-- Rotate -->
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
