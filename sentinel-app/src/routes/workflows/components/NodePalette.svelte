<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import Search from "@lucide/svelte/icons/search";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { portColor, typeColor } from "$lib/workflows/portColors";

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

  const EVENT_GROUP_PREFIX = "Events · ";
  const CATEGORY_ORDER = ["Cameras", "Vision", "AI", "Alerts", "Recipients", "Text", "Network", "Control", "Logic", "Recording", "Presence", "Debug"];

  let {
    open = $bindable(false),
    kinds,
    eventPaletteGroups = [],
    onSelect,
  }: {
    open: boolean;
    kinds: PaletteKind[];
    eventPaletteGroups?: { name: string; kinds: PaletteKind[] }[];
    onSelect: (slug: string, presetEvent?: string) => void;
  } = $props();

  let search = $state("");

  $effect(() => {
    if (!open) search = "";
  });

  const nodeCategories = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const matchKind = (k: PaletteKind) =>
      !q ||
      k.label.toLowerCase().includes(q) ||
      k.category.toLowerCase().includes(q) ||
      k.slug.toLowerCase().includes(q);

    const matched = kinds.filter((k) => k.slug !== "comment" && k.slug !== "reroute" && matchKind(k));
    const groups = new Map<string, PaletteKind[]>();
    for (const k of matched) {
      const list = groups.get(k.category) ?? [];
      list.push(k);
      groups.set(k.category, list);
    }

    const ordered: { name: string; kinds: PaletteKind[] }[] = [];
    for (const name of CATEGORY_ORDER) {
      const list = groups.get(name);
      if (list) {
        ordered.push({ name, kinds: list });
        groups.delete(name);
      }
    }
    for (const [name, list] of groups) ordered.push({ name, kinds: list });

    for (const g of eventPaletteGroups) {
      const ks = g.kinds.filter(matchKind);
      if (ks.length) ordered.push({ name: g.name, kinds: ks });
    }
    return ordered;
  });

  function close() {
    open = false;
    search = "";
  }

  function select(slug: string, presetEvent?: string) {
    close();
    onSelect(slug, presetEvent);
  }

  function portDots(outputs: CatPort[]) {
    return outputs.slice(0, 4).filter((p) => p.type !== "exec");
  }
</script>

{#if open}
  <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
    <button
      class="absolute inset-0 bg-background/40 backdrop-blur-sm duration-200 animate-in fade-in"
      aria-label="Close"
      onclick={close}
    ></button>
    <div
      class="relative z-10 flex max-h-[78vh] w-full max-w-md flex-col rounded-2xl border border-border bg-popover/95 p-4 shadow-2xl duration-200 animate-in fade-in zoom-in-95"
    >
      <div class="mb-3 flex items-center justify-between">
        <h3 class="flex items-center gap-2 text-sm font-semibold">
          <span class="size-1.5 rounded-full bg-primary"></span>
          Add a node
        </h3>
        <Button variant="ghost" size="icon" class="size-7" onclick={close}>
          <X class="size-4" />
        </Button>
      </div>

      <div class="relative mb-3">
        <Search class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <!-- svelte-ignore a11y_autofocus -->
        <Input autofocus bind:value={search} placeholder="Search nodes…" class="h-9 pl-8" />
      </div>

      <div class="-mr-2 flex-1 space-y-4 overflow-y-auto pr-2">
        {#each nodeCategories as cat (cat.name)}
          <div>
            <div class="mb-1.5 px-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {cat.name}
            </div>
            <div class="space-y-1">
              {#each cat.kinds as k (k.label)}
                <button
                  class="interactive group flex w-full items-center justify-between gap-3 rounded-xl border border-border bg-card px-3 py-2 text-left"
                  onclick={() => select(k.slug, k.presetEvent)}
                >
                  <span class="flex min-w-0 items-center gap-2 text-sm font-medium">
                    <span class="size-1.5 shrink-0 rounded-full bg-primary transition-transform group-hover:scale-125"></span>
                    <span class="truncate">{k.label}</span>
                  </span>
                  <span class="flex shrink-0 items-center gap-1.5">
                    <span class="text-[10px] tabular-nums text-muted-foreground">
                      {k.inputs.filter((p) => p.type !== "exec").length}→{k.outputs.filter((p) => p.type !== "exec").length}
                    </span>
                    <span class="flex items-center gap-0.5">
                      {#each portDots(k.outputs) as p}
                        <span
                          class="size-1.5 rounded-full opacity-75"
                          style:background-color={typeColor(p.type)}
                        ></span>
                      {/each}
                    </span>
                  </span>
                </button>
              {/each}
            </div>
          </div>
        {:else}
          <div class="flex flex-col items-center gap-2 py-10 text-center">
            <Search class="size-6 text-muted-foreground/50" />
            <p class="text-sm text-muted-foreground">No nodes match "{search}"</p>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}
