<script lang="ts">
  // Collapsible right panel for the Map editor. Two icon tabs — Properties (selection
  // editor) and Explorer (the item tree) — plus a collapse toggle like the workflow
  // editor. Open state + active tab persist in localStorage.
  import { onMount } from "svelte";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import PanelRightOpen from "@lucide/svelte/icons/panel-right-open";
  import PanelRightClose from "@lucide/svelte/icons/panel-right-close";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { ToolState } from "$lib/map/tools.svelte";
  import PropertiesPanel from "./PropertiesPanel.svelte";
  import ExplorerPanel from "./ExplorerPanel.svelte";

  let {
    store,
    tools,
    onGoLive,
  }: { store: MapStore; tools: ToolState; onGoLive: (ip: string) => void } = $props();

  type Tab = "properties" | "explorer";
  let tab = $state<Tab>("properties");
  let open = $state(true);

  onMount(() => {
    const t = localStorage.getItem("map.panelTab");
    if (t === "properties" || t === "explorer") tab = t;
    const o = localStorage.getItem("map.panelOpen");
    if (o != null) open = o === "true";
  });

  function setTab(t: Tab) {
    tab = t;
    try {
      localStorage.setItem("map.panelTab", t);
    } catch {
      // ignore storage failures (private mode)
    }
  }
  function toggle() {
    open = !open;
    try {
      localStorage.setItem("map.panelOpen", String(open));
    } catch {
      // ignore storage failures (private mode)
    }
  }
  function tabClass(active: boolean): string {
    return active
      ? "bg-primary/15 text-primary"
      : "text-muted-foreground hover:bg-muted/60 hover:text-foreground";
  }
</script>

{#if open}
  <aside class="flex w-[22rem] shrink-0 flex-col overflow-hidden rounded-xl border border-border bg-card">
    <div class="flex items-center gap-1 border-b border-border px-2 py-1.5">
      <button
        type="button"
        class="interactive inline-flex size-8 items-center justify-center rounded-md {tabClass(tab === 'properties')}"
        title="Properties"
        aria-label="Properties"
        aria-pressed={tab === "properties"}
        onclick={() => setTab("properties")}
      >
        <SlidersHorizontal class="size-4" />
      </button>
      <button
        type="button"
        class="interactive inline-flex size-8 items-center justify-center rounded-md {tabClass(tab === 'explorer')}"
        title="Explorer"
        aria-label="Explorer"
        aria-pressed={tab === "explorer"}
        onclick={() => setTab("explorer")}
      >
        <ListTree class="size-4" />
      </button>
      <button
        type="button"
        class="interactive ml-auto inline-flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted/60 hover:text-foreground"
        title="Collapse panel"
        aria-label="Collapse panel"
        onclick={toggle}
      >
        <PanelRightClose class="size-4" />
      </button>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      {#if tab === "properties"}
        <PropertiesPanel {store} {tools} {onGoLive} />
      {:else}
        <ExplorerPanel {store} />
      {/if}
    </div>
  </aside>
{:else}
  <div class="flex w-10 shrink-0 flex-col items-center rounded-xl border border-border bg-card py-2">
    <button
      type="button"
      class="interactive inline-flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted/60 hover:text-foreground"
      title="Show panel"
      aria-label="Show panel"
      onclick={toggle}
    >
      <PanelRightOpen class="size-4" />
    </button>
  </div>
{/if}
