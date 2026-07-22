<script lang="ts">
  // The Templates tab of the workflows list — a grid of the tenant's own templates plus every
  // global one. Self-contained + prop-driven (the parent owns the data + clone action). #102/#136/#137.
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Globe from "@lucide/svelte/icons/globe";
  import Pencil from "@lucide/svelte/icons/pencil";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import EmptyState from "$lib/components/empty-state.svelte";

  type TemplateSummary = { id: string; name: string; description: string; global: boolean; inputs: { name: string; type: string }[] };

  let {
    templates,
    loading,
    usingId,
    editableIds,
    onUse,
    onEdit,
    onBack,
  }: {
    templates: TemplateSummary[];
    loading: boolean;
    usingId: string | null;
    // Ids of templates that live in the CURRENT tenant → editable in place (own templates, or the
    // shared globals when the operator has assumed the platform tenant). Others are clone-only.
    editableIds: Set<string>;
    onUse: (t: TemplateSummary) => void;
    onEdit: (t: TemplateSummary) => void;
    onBack: () => void;
  } = $props();
</script>

<div class="min-h-0 flex-1 overflow-y-auto pt-1">
  {#if loading}
    <div class="p-12 text-center text-sm text-muted-foreground">Loading templates…</div>
  {:else if templates.length === 0}
    <div class="p-6">
      <EmptyState
        title="No templates yet"
        description="Save a workflow as a template (the bookmark on its row) and it'll appear here to reuse."
      >
        <Button variant="outline" onclick={onBack}>Back to my workflows</Button>
      </EmptyState>
    </div>
  {:else}
    <div class="grid gap-4 pb-2 sm:grid-cols-2 lg:grid-cols-3">
      {#each templates as t (t.id)}
        <div class="interactive flex flex-col rounded-2xl border border-border bg-card p-5">
          <div class="mb-3 flex items-start justify-between gap-3">
            <div class="flex size-9 items-center justify-center rounded-xl bg-primary/10 text-primary">
              <LayoutTemplate class="size-5" />
            </div>
            {#if t.global}
              <Badge variant="outline" class="border-primary/25 bg-primary/10 text-primary">
                <Globe class="mr-1 size-3" /> Global
              </Badge>
            {/if}
          </div>
          <h3 class="text-[15px] font-semibold tracking-tight">{t.name}</h3>
          {#if t.description}
            <p class="mt-1 line-clamp-2 text-sm text-muted-foreground">{t.description}</p>
          {/if}
          {#if t.inputs.length}
            <div class="mt-3 flex items-start gap-1.5 text-xs text-muted-foreground/80">
              <SlidersHorizontal class="mt-0.5 size-3.5 shrink-0" />
              <span>Asks for {t.inputs.map((v) => v.name).join(", ")}</span>
            </div>
          {/if}
          <div class="mt-4 flex items-center justify-end gap-2">
            {#if editableIds.has(t.id)}
              <Button variant="ghost" size="sm" onclick={() => onEdit(t)}>
                <Pencil class="size-3.5" /> Edit
              </Button>
            {/if}
            <Button size="sm" class="press" disabled={usingId === t.id} onclick={() => onUse(t)}>
              {usingId === t.id ? "Creating…" : "Use template"}
              {#if usingId !== t.id}<ArrowRight class="size-3.5" />{/if}
            </Button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
