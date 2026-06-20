<script lang="ts">
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import type { MapStore } from "$lib/map/store.svelte";

  let { store }: { store: MapStore } = $props();

  const t = $derived(store.selectedText);

  function setField(patch: Parameters<typeof store.updateText>[1]) {
    if (!t) return;
    store.updateText(t.id, patch);
  }
</script>

{#if t}
  <div class="space-y-3">
    <Label>Text label</Label>

    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Content</Label>
      <input
        type="text"
        value={t.text}
        class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
        oninput={(e) => setField({ text: e.currentTarget.value })}
      />
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <Label class="text-xs text-muted-foreground">Size</Label>
        <span class="text-xs tabular-nums text-muted-foreground">{(t.size ?? 0.5).toFixed(2)}</span>
      </div>
      <input
        type="range"
        min="0.1"
        max="3"
        step="0.05"
        value={t.size ?? 0.5}
        class="h-1.5 w-full accent-primary"
        onpointerdown={() => store.checkpoint()}
        oninput={(e) => setField({ size: e.currentTarget.valueAsNumber })}
      />
    </div>

    <div class="space-y-1.5">
      <Label class="text-xs text-muted-foreground">Color</Label>
      <div class="flex items-center gap-2">
        <input
          type="color"
          value={t.color ?? "#ffffff"}
          class="h-8 w-10 cursor-pointer rounded border border-border bg-transparent p-0.5"
          onpointerdown={() => store.checkpoint()}
          oninput={(e) => setField({ color: e.currentTarget.value })}
        />
        {#if t.color && t.color !== "#ffffff"}
          <Button variant="ghost" size="sm" class="h-8 text-xs text-muted-foreground"
            onclick={() => { store.checkpoint(); setField({ color: "#ffffff" }); }}>
            Reset
          </Button>
        {:else}
          <span class="text-xs text-muted-foreground">White</span>
        {/if}
      </div>
    </div>

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

    <Button variant="destructive" size="sm" class="w-full" onclick={() => store.deleteText(t.id)}>
      <Trash2 class="size-4" /> Delete label
    </Button>
  </div>
{/if}
