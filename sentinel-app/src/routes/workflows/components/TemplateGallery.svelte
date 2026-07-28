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
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].workflowEditor);
</script>

{#if loading}
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    <div class="p-12 text-center text-sm text-muted-foreground">{t.loadingTemplates}</div>
  </div>
{:else if templates.length === 0}
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    <div class="p-6">
      <EmptyState
        title={t.noTemplatesTitle}
        description="Save a workflow as a template (the bookmark on its row) and it'll appear here to reuse."
      >
        <Button variant="outline" onclick={onBack}>{t.backToMine}</Button>
      </EmptyState>
    </div>
  </div>
{:else}
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    <div class="divide-y divide-border/60">
      {#each templates as tpl (tpl.id)}
        <div class="group flex items-center gap-3 px-3 py-2.5 transition-colors hover:bg-primary/5">
          <div class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <LayoutTemplate class="size-4" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-medium">{tpl.name}</span>
              {#if tpl.global}
                <Badge variant="outline" class="shrink-0 border-primary/25 bg-primary/10 text-primary">
                  <Globe class="mr-1 size-3" /> Global
                </Badge>
              {/if}
            </div>
            {#if tpl.description}<p class="truncate text-xs text-muted-foreground">{tpl.description}</p>{/if}
            {#if tpl.inputs.length}
              <p class="mt-0.5 truncate text-[11px] text-muted-foreground/70">Asks for {tpl.inputs.map((v) => v.name).join(", ")}</p>
            {/if}
          </div>
          <Button size="sm" class="press shrink-0" disabled={usingId === tpl.id} onclick={() => onUse(tpl)}>
            {usingId === tpl.id ? t.creating : t.useTemplate}
            {#if usingId !== tpl.id}<ArrowRight class="size-3.5" />{/if}
          </Button>
        </div>
      {/each}
    </div>
  </div>
{/if}
