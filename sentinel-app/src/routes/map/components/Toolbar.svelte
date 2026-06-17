<script lang="ts">
  // Map header: title, the drawing-tool palette (Select / Wall), undo/redo, Save.
  import Save from "@lucide/svelte/icons/save";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import Spline from "@lucide/svelte/icons/spline";
  import Armchair from "@lucide/svelte/icons/armchair";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import { Button } from "$lib/components/ui/button";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { ToolMode, ToolState } from "$lib/map/tools";

  let {
    store,
    tools,
    onSave,
  }: { store: MapStore; tools: ToolState; onSave: () => void } = $props();

  function toolVariant(m: ToolMode): "default" | "ghost" {
    return tools.mode === m ? "default" : "ghost";
  }
</script>

<div class="flex items-center justify-between gap-3">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Map</h1>
    <p class="text-sm text-muted-foreground">
      Top-down floor plan — draw walls, place cameras, and jump to Live.
    </p>
  </div>

  <div class="flex items-center gap-2">
    <!-- tool palette -->
    <div class="flex items-center gap-1 rounded-lg border border-border bg-card/50 p-1">
      <Button variant={toolVariant("select")} size="icon" class="size-8" title="Select / move (V)" onclick={() => tools.setMode("select")}>
        <MousePointer2 class="size-4" />
      </Button>
      <Button variant={toolVariant("wall")} size="icon" class="size-8" title="Draw wall (W)" onclick={() => tools.setMode("wall")}>
        <Spline class="size-4" />
      </Button>
      <Button variant={toolVariant("marker")} size="icon" class="size-8" title="Place marker (M)" onclick={() => tools.setMode("marker")}>
        <Armchair class="size-4" />
      </Button>
    </div>

    <!-- undo / redo -->
    <div class="flex items-center gap-1 rounded-lg border border-border bg-card/50 p-1">
      <Button variant="ghost" size="icon" class="size-8" title="Undo (⌘Z)" disabled={!store.canUndo} onclick={() => store.undo()}>
        <Undo2 class="size-4" />
      </Button>
      <Button variant="ghost" size="icon" class="size-8" title="Redo (⇧⌘Z)" disabled={!store.canRedo} onclick={() => store.redo()}>
        <Redo2 class="size-4" />
      </Button>
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
</div>
