<script lang="ts">
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import * as Select from "$lib/components/ui/select";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { StrokeStyle } from "$lib/types";

  let { store }: { store: MapStore } = $props();

  const s = $derived(store.selectedShape);

  const STROKE_STYLES: { value: StrokeStyle; label: string }[] = [
    { value: "solid",  label: "Solid" },
    { value: "dashed", label: "Dashed" },
    { value: "dotted", label: "Dotted" },
  ];

  const strokeStyleLabel = $derived(
    STROKE_STYLES.find((x) => x.value === (s?.strokeStyle ?? "solid"))?.label ?? "Solid"
  );

  const kindLabel: Record<string, string> = { rect: "Rectangle", triangle: "Triangle", circle: "Circle" };

  function patch(p: Parameters<typeof store.updateShape>[1]) {
    if (!s) return;
    store.updateShape(s.id, p);
  }

  function setColor(v: string) { patch({ color: v }); }
  function setFill(v: string) { patch({ fill: v }); }

  let _pendingStroke: string | null = null;
  let _strokeRaf: number | null = null;
  function onStrokeInput(v: string) {
    _pendingStroke = v;
    if (_strokeRaf !== null) return;
    _strokeRaf = requestAnimationFrame(() => {
      _strokeRaf = null;
      if (_pendingStroke !== null) { setColor(_pendingStroke); _pendingStroke = null; }
    });
  }

  let _pendingFill: string | null = null;
  let _fillRaf: number | null = null;
  function onFillInput(v: string) {
    _pendingFill = v;
    if (_fillRaf !== null) return;
    _fillRaf = requestAnimationFrame(() => {
      _fillRaf = null;
      if (_pendingFill !== null) { setFill(_pendingFill); _pendingFill = null; }
    });
  }
  function clearFill() { store.checkpoint(); patch({ fill: undefined }); }
</script>

{#if s}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <Label>{kindLabel[s.kind] ?? s.kind}</Label>
    </div>

    <!-- Stroke color -->
    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Stroke color</Label>
      <div class="flex items-center gap-2">
        <input
          type="color"
          value={s.color ?? "#ffffff"}
          class="h-8 w-10 cursor-pointer rounded border border-border bg-transparent p-0.5"
          onpointerdown={() => store.checkpoint()}
          oninput={(e) => onStrokeInput(e.currentTarget.value)}
        />
        {#if s.color && s.color !== "#ffffff"}
          <Button variant="ghost" size="sm" class="h-8 text-xs text-muted-foreground"
            onclick={() => { store.checkpoint(); patch({ color: "#ffffff" }); }}>
            Reset
          </Button>
        {:else}
          <span class="text-xs text-muted-foreground">White</span>
        {/if}
      </div>
    </div>

    <!-- Stroke style -->
    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Stroke style</Label>
      <Select.Root
        type="single"
        value={s.strokeStyle ?? "solid"}
        onValueChange={(v) => { store.checkpoint(); patch({ strokeStyle: v as StrokeStyle }); }}
      >
        <Select.Trigger class="w-full">{strokeStyleLabel}</Select.Trigger>
        <Select.Content>
          {#each STROKE_STYLES as st (st.value)}
            <Select.Item value={st.value} label={st.label}>{st.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    <!-- Fill color -->
    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Fill</Label>
      <div class="flex items-center gap-2">
        <input
          type="color"
          value={s.fill ?? "#ffffff"}
          class="h-8 w-10 cursor-pointer rounded border border-border bg-transparent p-0.5"
          onpointerdown={() => store.checkpoint()}
          oninput={(e) => onFillInput(e.currentTarget.value)}
        />
        {#if s.fill}
          <Button variant="ghost" size="sm" class="h-8 text-xs text-muted-foreground" onclick={clearFill}>
            No fill
          </Button>
        {:else}
          <Button variant="ghost" size="sm" class="h-8 text-xs text-muted-foreground"
            onclick={() => { store.checkpoint(); patch({ fill: "#ffffff" }); }}>
            Add fill
          </Button>
        {/if}
      </div>
    </div>

    <!-- Rotation (not applicable for triangles) -->
    {#if s.kind !== "triangle"}
      <div class="space-y-1.5">
        <Label class="text-xs text-muted-foreground">Rotate</Label>
        <div class="flex items-center gap-2">
          <Button variant="outline" size="sm" class="flex-1" onclick={() => store.rotateSelected(-15)}>
            <RotateCcw class="size-4" /> −15°
          </Button>
          <Button variant="outline" size="sm" class="flex-1" onclick={() => store.rotateSelected(15)}>
            <RotateCw class="size-4" /> +15°
          </Button>
        </div>
      </div>
    {/if}

    <Button variant="destructive" size="sm" class="w-full" onclick={() => store.deleteShape(s.id)}>
      <Trash2 class="size-4" /> Delete shape
    </Button>
  </div>
{/if}
