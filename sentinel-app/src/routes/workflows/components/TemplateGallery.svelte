<script lang="ts">
  // The Templates tab of the workflows list — a row list (matching My workflows) of the tenant's
  // own templates plus every global one. Prop-driven; the parent owns the data + clone action.
  // #102/#136/#137.
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Globe from "@lucide/svelte/icons/globe";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import EmptyState from "$lib/components/empty-state.svelte";

  type TemplateSummary = { id: string; name: string; description: string; global: boolean; inputs: { name: string; type: string }[] };

  let {
    templates,
    loading,
    usingId,
    onUse,
    onBack,
  }: {
    templates: TemplateSummary[];
    loading: boolean;
    usingId: string | null;
    onUse: (t: TemplateSummary) => void;
    onBack: () => void;
  } = $props();
</script>

{#if loading}
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    <div class="p-12 text-center text-sm text-muted-foreground">Loading templates…</div>
  </div>
{:else if templates.length === 0}
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    <div class="p-6">
      <EmptyState
        title="No templates yet"
        description="Save a workflow as a template (the bookmark on its row) and it'll appear here to reuse."
      >
        <Button variant="outline" onclick={onBack}>Back to my workflows</Button>
      </EmptyState>
    </div>
  </div>
{:else}
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    <div class="divide-y divide-border/60">
      {#each templates as t (t.id)}
        <div class="group flex items-center gap-3 px-3 py-2.5 transition-colors hover:bg-primary/5">
          <div class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <LayoutTemplate class="size-4" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-medium">{t.name}</span>
              {#if t.global}
                <Badge variant="outline" class="shrink-0 border-primary/25 bg-primary/10 text-primary">
                  <Globe class="mr-1 size-3" /> Global
                </Badge>
              {/if}
            </div>
            {#if t.description}<p class="truncate text-xs text-muted-foreground">{t.description}</p>{/if}
            {#if t.inputs.length}
              <p class="mt-0.5 truncate text-[11px] text-muted-foreground/70">Asks for {t.inputs.map((v) => v.name).join(", ")}</p>
            {/if}
          </div>
          <Button size="sm" class="press shrink-0" disabled={usingId === t.id} onclick={() => onUse(t)}>
            {usingId === t.id ? "Creating…" : "Use template"}
            {#if usingId !== t.id}<ArrowRight class="size-3.5" />{/if}
          </Button>
        </div>
      {/each}
    </div>
  </div>
{/if}
