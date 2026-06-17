<script lang="ts">
  // Unit 018 — Map. Thin shell: owns the MapStore + Viewport, composes the header,
  // the Threlte canvas (with the zoom/fit overlay), and the editor panel. All the
  // room/camera logic lives in the store; all the rendering lives in lib/three/map.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import { Canvas } from "@threlte/core";

  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import Maximize from "@lucide/svelte/icons/maximize";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import Loader from "$lib/components/loader.svelte";

  import { MapStore } from "$lib/map/store.svelte";
  import { Viewport } from "$lib/map/view3d";
  import { ToolState } from "$lib/map/tools";
  import { handleMapKey } from "$lib/map/keyboard";
  import MapScene from "$lib/three/map/MapScene.svelte";
  import Toolbar from "./components/Toolbar.svelte";
  import EditorPanel from "./components/EditorPanel.svelte";

  const store = new MapStore();
  const viewport = new Viewport();
  const tools = new ToolState();

  function bounds() {
    if (store.rooms.length === 0 && store.walls.length === 0) return null;
    let minX = Infinity,
      maxX = -Infinity,
      minZ = Infinity,
      maxZ = -Infinity;
    const ext = (x: number, z: number) => {
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minZ = Math.min(minZ, z);
      maxZ = Math.max(maxZ, z);
    };
    for (const r of store.rooms) {
      ext(r.x, r.y);
      ext(r.x + r.w, r.y + r.h);
    }
    for (const w of store.walls) {
      ext(w.x1, w.y1);
      ext(w.x2, w.y2);
    }
    return { minX, maxX, minZ, maxZ };
  }

  function fit() {
    viewport.fit(bounds());
  }

  function goLive(ip: string) {
    goto(`/live?cam=${encodeURIComponent(ip)}`);
  }

  async function save() {
    try {
      await store.save();
      toast.success("Map saved.");
    } catch (err) {
      console.error("Failed to save map", err);
      toast.error("Failed to save map", { description: String(err) });
    }
  }

  function onKey(e: KeyboardEvent) {
    handleMapKey(e, { store, tools, fit });
  }

  $effect(() => {
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  onMount(async () => {
    try {
      await store.load();
      fit();
    } catch (err) {
      console.error("Failed to load map", err);
      toast.error("Failed to load map", { description: String(err) });
    }
  });
</script>

<div class="flex h-[calc(100vh-7rem)] flex-col gap-4">
  <Toolbar {store} {tools} onSave={save} />

  {#if store.loading}
    <Loader label="Loading map…" />
  {:else}
    <div class="grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-[1fr_22rem]">
      <!-- 3D canvas (top-down) -->
      <Card.Root class="relative min-h-[24rem] overflow-hidden p-0">
        <div class="h-full w-full bg-[oklch(0.165_0.006_160)]">
          <Canvas>
            <MapScene {store} {viewport} {tools} onCameraClick={goLive} />
          </Canvas>
        </div>

        <!-- zoom / fit overlay -->
        <div
          class="absolute right-3 top-3 flex items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
        >
          <Button variant="ghost" size="icon" class="size-8" title="Zoom out" onclick={() => viewport.zoomBy(1 / 1.2)}>
            <ZoomOut class="size-4" />
          </Button>
          <span class="w-10 text-center text-[11px] tabular-nums text-muted-foreground">
            {Math.round(viewport.zoom * 100)}%
          </span>
          <Button variant="ghost" size="icon" class="size-8" title="Zoom in" onclick={() => viewport.zoomBy(1.2)}>
            <ZoomIn class="size-4" />
          </Button>
          <Button variant="ghost" size="icon" class="size-8" title="Fit to view" onclick={fit}>
            <Maximize class="size-4" />
          </Button>
        </div>
      </Card.Root>

      <EditorPanel {store} {tools} onGoLive={goLive} />
    </div>
  {/if}
</div>
