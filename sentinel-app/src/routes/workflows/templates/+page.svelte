<script lang="ts">
  // Templates gallery — a full view (not a modal). Browse the tenant's own templates plus every
  // global one, and clone one into a runnable workflow. Card #102.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Globe from "@lucide/svelte/icons/globe";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import PageHeader from "$lib/components/page-header.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Loader from "$lib/components/loader.svelte";
  import IconChip from "$lib/components/icon-chip.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { data } from "$lib/data";

  type TemplateSummary = {
    id: string;
    name: string;
    description: string;
    global: boolean;
    inputs: { name: string; type: string }[];
  };

  let templates = $state<TemplateSummary[]>([]);
  let loading = $state(true);
  let usingId = $state<string | null>(null);

  onMount(load);
  async function load() {
    loading = true;
    try {
      templates = await data.listTemplates<TemplateSummary>();
    } catch (err) {
      toast.error("Couldn't load templates", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  // Clone a template into this tenant, then open the new workflow to fill in its inputs.
  async function use(t: TemplateSummary) {
    usingId = t.id;
    try {
      const id = await data.useTemplate(t.id);
      toast.info(`Created “${t.name}” — fill in its inputs and turn it on.`);
      goto(`/workflows/${id}`);
    } catch (err) {
      usingId = null;
      toast.error("Couldn't use template", { description: String(err) });
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Templates"
    description="Ready-made workflows to start from — cloned into your workspace to fill in and turn on."
    icon={LayoutTemplate}
  >
    {#snippet actions()}
      <Button variant="outline" onclick={() => goto("/workflows")}>
        <ArrowLeft class="size-4" /> Workflows
      </Button>
    {/snippet}
  </PageHeader>

  {#if loading}
    <Loader label="Loading templates…" />
  {:else if templates.length === 0}
    <EmptyState
      title="No templates yet"
      description="Save a workflow as a template with the bookmark on its row, and it'll appear here to reuse."
    >
      {#snippet children()}
        <Button variant="outline" onclick={() => goto("/workflows")}>
          <ArrowLeft class="size-4" /> Back to workflows
        </Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each templates as t (t.id)}
        <div class="interactive flex flex-col rounded-2xl border border-border bg-card p-5">
          <div class="mb-3 flex items-start justify-between gap-3">
            <IconChip>
              {#snippet children()}<LayoutTemplate class="size-5" />{/snippet}
            </IconChip>
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
          <div class="mt-4 flex justify-end">
            <Button size="sm" class="press" disabled={usingId === t.id} onclick={() => use(t)}>
              {usingId === t.id ? "Creating…" : "Use template"}
              {#if usingId !== t.id}<ArrowRight class="size-3.5" />{/if}
            </Button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
