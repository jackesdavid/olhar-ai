<script lang="ts">
  // Instances — a full view (not a modal): a graph's live run contexts and the saved value of every
  // node in each. Master/detail: pick an instance on the left, read its per-node values on the right.
  // Card #95. Graph selected via ?graph=<id>&name=<name>.
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import Layers from "@lucide/svelte/icons/layers";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import PageHeader from "$lib/components/page-header.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Loader from "$lib/components/loader.svelte";
  import { Button } from "$lib/components/ui/button";
  import { data, type WorkflowInstance, type InstanceValues } from "$lib/data";

  const graphId = $derived($page.url.searchParams.get("graph") ?? "");
  const graphName = $derived($page.url.searchParams.get("name") ?? "");

  let instances = $state<WorkflowInstance[]>([]);
  let loading = $state(true);
  let selected = $state<string | null>(null);
  let values = $state<InstanceValues | null>(null);
  let valuesLoading = $state(false);

  onMount(load);
  async function load() {
    if (!graphId) {
      loading = false;
      return;
    }
    loading = true;
    try {
      instances = await data.listInstances(graphId);
      // Auto-open the sole instance (the common Single-scope case) so there's no dead click.
      if (instances.length === 1) select(instances[0].instance_key);
      else if (selected && !instances.some((i) => i.instance_key === selected)) selected = null;
    } catch (err) {
      toast.error("Couldn't load instances", { description: String(err) });
    } finally {
      loading = false;
    }
  }
  async function select(key: string) {
    selected = key;
    values = null;
    valuesLoading = true;
    try {
      values = await data.getInstanceValues(graphId, key);
    } catch (err) {
      toast.error("Couldn't load instance values", { description: String(err) });
    } finally {
      valuesLoading = false;
    }
  }
  function label(key: string): string {
    return key === "" ? "Default (single)" : key;
  }
  function fmtTs(ts: string | null): string {
    if (!ts) return "";
    const d = new Date(ts);
    return Number.isNaN(d.getTime()) ? "" : d.toLocaleString();
  }
</script>

<div class="flex h-full flex-col gap-6">
  <PageHeader
    title={graphName ? `Instances — ${graphName}` : "Instances"}
    description="Each run context that has saved data, and the last value every node produced in it."
    icon={Layers}
  >
    {#snippet actions()}
      <Button variant="outline" onclick={() => goto("/workflows")}>
        <ArrowLeft class="size-4" /> Workflows
      </Button>
      <Button variant="outline" onclick={load} disabled={loading}>
        <RefreshCw class="size-4 {loading ? 'animate-spin' : ''}" /> Refresh
      </Button>
    {/snippet}
  </PageHeader>

  {#if !graphId}
    <EmptyState title="No workflow selected" description="Open a workflow's instances from its row on the Workflows page.">
      {#snippet children()}
        <Button variant="outline" onclick={() => goto("/workflows")}>
          <ArrowLeft class="size-4" /> Back to workflows
        </Button>
      {/snippet}
    </EmptyState>
  {:else if loading}
    <Loader label="Loading instances…" />
  {:else if instances.length === 0}
    <EmptyState
      title="No saved run contexts yet"
      description="Run this workflow, then its instances and their saved node values appear here."
    >
      {#snippet children()}
        <Button variant="outline" onclick={() => goto("/workflows")}>
          <ArrowLeft class="size-4" /> Back to workflows
        </Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="grid min-h-0 flex-1 grid-cols-[minmax(0,18rem)_minmax(0,1fr)] gap-4">
      <!-- instance list -->
      <div class="min-h-0 space-y-1.5 overflow-y-auto pr-1">
        {#each instances as inst (inst.instance_key)}
          <button
            class="interactive flex w-full items-center gap-2 rounded-xl border px-3 py-2.5 text-left {selected === inst.instance_key
              ? 'border-primary bg-primary/10'
              : 'border-border bg-card'}"
            onclick={() => select(inst.instance_key)}
          >
            {#if inst.active}<span class="status-live size-2 shrink-0 rounded-full bg-primary"></span>{/if}
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm font-medium" title={label(inst.instance_key)}>{label(inst.instance_key)}</span>
              {#if inst.status}<span class="block text-[11px] text-muted-foreground">{inst.status}{#if inst.updated_at} · {fmtTs(inst.updated_at)}{/if}</span>{/if}
            </span>
          </button>
        {/each}
      </div>

      <!-- selected instance's per-node values -->
      <div class="min-h-0 overflow-y-auto rounded-2xl border border-border bg-card p-5">
        {#if !selected}
          <p class="py-12 text-center text-sm text-muted-foreground">Pick an instance to see its saved values.</p>
        {:else if valuesLoading}
          <Loader label="Loading values…" />
        {:else if !values || values.nodes.length === 0}
          <p class="py-12 text-center text-sm text-muted-foreground">No node values saved in this instance.</p>
        {:else}
          <div class="space-y-4">
            {#each values.nodes as n (n.node_id)}
              <div>
                <div class="mb-1.5 text-sm font-semibold tracking-tight">{n.label ?? `Node ${n.node_id}`}</div>
                {#if Object.keys(n.values).length === 0}
                  <p class="text-[11px] text-muted-foreground/70">— no outputs —</p>
                {:else}
                  <div class="space-y-1 font-mono text-xs">
                    {#each Object.entries(n.values) as [port, val] (port)}
                      <div class="flex gap-3">
                        <span class="w-32 shrink-0 truncate text-muted-foreground" title={port}>{port}</span>
                        <span class="min-w-0 flex-1 break-all" title={val}>{val}</span>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>
