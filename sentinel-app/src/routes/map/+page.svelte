<script lang="ts">
  // Unit 018 — Map. Thin shell: owns the MapStore + Viewport + tools, composes the
  // header, the full-bleed Threlte canvas (with floating overlays: undo/redo top-left,
  // zoom/fit top-right, the tool palette center-right, presence legend bottom-left),
  // and the collapsible Properties/Explorer side panel. Logic lives in the store; the
  // rendering lives in lib/three/map.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import { Canvas } from "@threlte/core";

  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import Spline from "@lucide/svelte/icons/spline";
  import Minus from "@lucide/svelte/icons/minus";
  import Armchair from "@lucide/svelte/icons/armchair";
  import Type from "@lucide/svelte/icons/type";
  import Square from "@lucide/svelte/icons/square";
  import Triangle from "@lucide/svelte/icons/triangle";
  import Circle from "@lucide/svelte/icons/circle";
  import Save from "@lucide/svelte/icons/save";
  import Busy from "$lib/components/busy.svelte";
  import Keyboard from "@lucide/svelte/icons/keyboard";

  import { Button } from "$lib/components/ui/button";
  import Loader from "$lib/components/loader.svelte";

  import { MapStore } from "$lib/map/store.svelte";
  import { Viewport } from "$lib/map/view3d.svelte";
  import { ToolState, type ToolMode } from "$lib/map/tools.svelte";
  import { PresenceStore } from "$lib/map/presence.svelte";
  import { CATEGORY_ORDER, CATEGORY_STYLE } from "$lib/map/categories";
  import { handleMapKey } from "$lib/map/keyboard";
  import MapScene from "$lib/three/map/MapScene.svelte";
  import SidePanel from "./components/SidePanel.svelte";
  import ShortcutsOverlay from "./components/ShortcutsOverlay.svelte";

  const store = new MapStore();
  const viewport = new Viewport();
  const tools = new ToolState();
  const presence = new PresenceStore(store);

  function toolVariant(m: ToolMode): "default" | "ghost" {
    return tools.mode === m ? "default" : "ghost";
  }

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

  let showHelp = $state(false);

  function onKey(e: KeyboardEvent) {
    handleMapKey(e, { store, tools, fit, toggleHelp: () => (showHelp = !showHelp) });
  }

  $effect(() => {
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  $effect(() => {
    presence.start();
    return () => presence.stop();
  });

  // Camera zoom/pan are preserved across sessions (localStorage). On first load: restore
  // the saved view if present, else fit once the canvas has real dimensions (the initial
  // fit can't run before the Threlte canvas is measured). After that the view is the
  // user's to control — F re-fits on demand.
  const VIEW_KEY = "map.viewport";
  let viewInit = false;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  function readSavedView(): { zoom: number; panX: number; panZ: number } | null {
    try {
      const v = JSON.parse(localStorage.getItem(VIEW_KEY) ?? "null");
      if (v && typeof v.zoom === "number" && typeof v.panX === "number" && typeof v.panZ === "number") return v;
    } catch {
      // ignore parse / storage errors
    }
    return null;
  }

  $effect(() => {
    if (viewInit || store.loading) return;
    const saved = readSavedView();
    if (saved) {
      viewport.zoom = saved.zoom;
      viewport.panX = saved.panX;
      viewport.panZ = saved.panZ;
      viewInit = true;
    } else if (viewport.width > 1 && viewport.height > 1) {
      fit();
      viewInit = true;
    }
  });

  // Persist the view (debounced) once it's initialized, so it survives reloads.
  $effect(() => {
    const snapshot = { zoom: viewport.zoom, panX: viewport.panX, panZ: viewport.panZ };
    if (!viewInit) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      try {
        localStorage.setItem(VIEW_KEY, JSON.stringify(snapshot));
      } catch {
        // ignore storage errors
      }
    }, 400);
  });

  onMount(async () => {
    try {
      await store.load();
      // Initial framing (restore saved view or fit) is handled reactively once the
      // canvas has real dimensions — see the view-init effect above.
    } catch (err) {
      console.error("Failed to load map", err);
      toast.error("Failed to load map", { description: String(err) });
    }
  });
</script>

<div class="flex h-full flex-col gap-3 p-4">
  <!-- header -->
  <div class="flex items-center justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Map</h1>
      <p class="text-sm text-muted-foreground">
        Top-down floor plan — draw walls, place cameras, and jump to Live.
      </p>
    </div>
    <Button onclick={save} disabled={store.saving || !store.dirty}>
      {#if store.saving}
        <Busy />
        Saving…
      {:else}
        <Save class="size-4" />
        Save{store.dirty ? " *" : ""}
      {/if}
    </Button>
  </div>

  {#if store.loading}
    <Loader label="Loading map…" />
  {:else}
    <div class="flex min-h-0 flex-1 gap-3">
      <!-- 3D canvas (top-down) -->
      <div class="relative min-h-0 flex-1 overflow-hidden rounded-xl border border-border bg-well">
        <div class="h-full w-full bg-well">
          <Canvas>
            <MapScene {store} {viewport} {tools} {presence} onCameraClick={goLive} />
          </Canvas>
        </div>

        <!-- undo / redo (top-left) -->
        <div class="absolute left-3 top-3 flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
          <Button variant="ghost" size="icon" class="size-8" title="Undo (⌘Z)" disabled={!store.canUndo} onclick={() => store.undo()}>
            <Undo2 class="size-4" />
          </Button>
          <Button variant="ghost" size="icon" class="size-8" title="Redo (⇧⌘Z)" disabled={!store.canRedo} onclick={() => store.redo()}>
            <Redo2 class="size-4" />
          </Button>
          <Button variant="ghost" size="icon" class="size-8" title="Keyboard shortcuts (?)" onclick={() => (showHelp = !showHelp)}>
            <Keyboard class="size-4" />
          </Button>
        </div>

        <!-- zoom / fit (top-right) -->
        <div class="absolute right-3 top-3 flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
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

        <!-- tool palette (center-right) -->
        <div class="absolute right-3 top-1/2 flex -translate-y-1/2 flex-col items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
          <Button variant={toolVariant("select")} size="icon" class="size-9" title="Select / move (V)" onclick={() => tools.setMode("select")}>
            <MousePointer2 class="size-4" />
          </Button>
          <div class="my-0.5 h-px w-6 bg-border"></div>
          <Button variant={toolVariant("wall")} size="icon" class="size-9" title="Draw wall (W)" onclick={() => tools.setMode("wall")}>
            <Spline class="size-4" />
          </Button>
          <Button variant={toolVariant("line")} size="icon" class="size-9" title="Draw line (L)" onclick={() => tools.setMode("line")}>
            <Minus class="size-4" />
          </Button>
          <div class="my-0.5 h-px w-6 bg-border"></div>
          <Button variant={toolVariant("rect")} size="icon" class="size-9" title="Rectangle (R)" onclick={() => tools.setMode("rect")}>
            <Square class="size-4" />
          </Button>
          <Button variant={toolVariant("triangle")} size="icon" class="size-9" title="Triangle (T)" onclick={() => tools.setMode("triangle")}>
            <Triangle class="size-4" />
          </Button>
          <Button variant={toolVariant("circle")} size="icon" class="size-9" title="Circle (C)" onclick={() => tools.setMode("circle")}>
            <Circle class="size-4" />
          </Button>
          <div class="my-0.5 h-px w-6 bg-border"></div>
          <Button variant={toolVariant("text")} size="icon" class="size-9" title="Text label" onclick={() => tools.setMode("text")}>
            <Type class="size-4" />
          </Button>
          <Button variant={toolVariant("marker")} size="icon" class="size-9" title="Place furniture (M)" onclick={() => tools.setMode("marker")}>
            <Armchair class="size-4" />
          </Button>
        </div>

        <!-- presence legend (bottom-left) -->
        <div class="absolute bottom-3 left-3 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-border bg-hud px-2.5 py-1.5 text-[11px] text-muted-foreground shadow-lg backdrop-blur">
          {#each CATEGORY_ORDER as cat (cat)}
            <span class="flex items-center gap-1.5">
              <span class="size-2 rounded-full" style="background:{CATEGORY_STYLE[cat].color}"></span>
              {CATEGORY_STYLE[cat].label}
            </span>
          {/each}
        </div>

        <ShortcutsOverlay open={showHelp} onClose={() => (showHelp = false)} />
      </div>

      <SidePanel {store} {tools} onGoLive={goLive} />
    </div>
  {/if}
</div>
