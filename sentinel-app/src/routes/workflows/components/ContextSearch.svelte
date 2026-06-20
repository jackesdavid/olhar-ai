<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import { Input } from "$lib/components/ui/input";
  import { portColor, typeColor } from "$lib/workflows/portColors";
  import type { PortType } from "$lib/workflows/portColors";

  type CatPort = { name: string; type: string; required: boolean };
  type PaletteKind = {
    slug: string;
    label: string;
    category: string;
    inputs: CatPort[];
    outputs: CatPort[];
    default_config: Record<string, unknown>;
    presetEvent?: string;
  };

  let {
    open = $bindable(false),
    pos,
    portType,
    kinds,
    onSelect,
  }: {
    open: boolean;
    pos: { x: number; y: number };
    portType: PortType | "any";
    kinds: PaletteKind[];
    onSelect: (slug: string, presetEvent?: string) => void;
  } = $props();

  let search = $state("");

  // Reroute is UI-only and not in the server catalog; inject it here so it's always available.
  const REROUTE_ENTRY: PaletteKind = {
    slug: "reroute",
    label: "Reroute",
    category: "Utility",
    inputs: [{ name: "value", type: "any", required: false }],
    outputs: [{ name: "value", type: "any", required: false }],
    default_config: {},
  };

  const filteredKinds = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const type = portType;
    const base = [REROUTE_ENTRY, ...kinds.filter((k) => k.slug !== "comment" && k.slug !== "reroute")];
    return base
      .filter((k) => !q || k.label.toLowerCase().includes(q) || k.slug.includes(q))
      .map((k) => ({
        ...k,
        _score:
          k.slug === "reroute"
            ? 10
            : k.inputs.some((p) => p.type === type)
              ? 2
              : k.inputs.some((p) => p.type === "any")
                ? 1
                : 0,
      }))
      .sort((a, b) => b._score - a._score || a.label.localeCompare(b.label));
  });

  function close() {
    open = false;
    search = "";
  }

  function select(slug: string, presetEvent?: string) {
    close();
    onSelect(slug, presetEvent);
  }

  function dotColor(k: PaletteKind & { _score: number }): string | null {
    if (k.slug === "reroute") return portColor.any;
    if (k.inputs.some((p) => p.type === portType)) return portColor[portType as PortType] ?? portColor.any;
    return null;
  }

  const panelLeft = $derived(
    typeof window !== "undefined" ? Math.min(pos.x, window.innerWidth - 280) : pos.x,
  );
  const panelTop = $derived(
    typeof window !== "undefined" ? Math.min(pos.y, window.innerHeight - 380) : pos.y,
  );
</script>

{#if open}
  <!-- Backdrop to close when clicking outside -->
  <button
    class="fixed inset-0 z-[59]"
    aria-label="Close"
    onclick={close}
  ></button>

  <div
    class="fixed z-[60] flex flex-col rounded-xl border border-border bg-popover/95 shadow-xl backdrop-blur-sm duration-150 animate-in fade-in zoom-in-95"
    style:left="{panelLeft}px"
    style:top="{panelTop}px"
    style:width="264px"
    style:max-height="360px"
  >
    <div class="flex items-center gap-2 border-b border-border px-3 py-2">
      <span
        class="size-2 shrink-0 rounded-full"
        style:background-color={portColor[portType as PortType] ?? portColor.any}
      ></span>
      <span class="flex-1 truncate text-xs font-medium text-muted-foreground">Connect to…</span>
      <button class="text-muted-foreground hover:text-foreground" onclick={close}>
        <X class="size-3.5" />
      </button>
    </div>

    <div class="px-2 py-1.5">
      <!-- svelte-ignore a11y_autofocus -->
      <Input autofocus bind:value={search} placeholder="Search…" class="h-7 text-xs" />
    </div>

    <div class="flex-1 overflow-y-auto px-2 pb-2">
      {#each filteredKinds as k (k.slug + (k.presetEvent ?? ""))}
        {@const dot = dotColor(k)}
        <button
          class="interactive group flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-accent"
          onclick={() => select(k.slug, k.presetEvent)}
        >
          {#if dot}
            <span class="size-2 shrink-0 rounded-full" style:background-color={dot}></span>
          {:else}
            <span class="size-2 shrink-0 rounded-full border border-border bg-transparent"></span>
          {/if}
          <span class="min-w-0 flex-1 truncate text-xs font-medium">{k.label}</span>
          <span class="flex shrink-0 items-center gap-0.5">
            {#each k.outputs.slice(0, 3) as p}
              <span class="size-1.5 rounded-full opacity-60" style:background-color={typeColor(p.type)}></span>
            {/each}
          </span>
        </button>
      {:else}
        <p class="py-4 text-center text-xs text-muted-foreground">No matches</p>
      {/each}
    </div>
  </div>
{/if}
