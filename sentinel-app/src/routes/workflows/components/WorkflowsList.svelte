<script lang="ts">
  // The workflows LIST mode — header, tabs (My workflows / Templates / Dictionary Tables),
  // search+group filter, grouped rows with per-row actions. Prop-driven: the parent
  // ([[id]]/+page.svelte) owns all state, SSE activity, the run/delete dialogs, and the row
  // actions; this component only renders and calls back. Extracted from the 5.5k-line
  // mega-component (#137, phase 3). No behaviour change.
  import { goto } from "$app/navigation";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Switch } from "$lib/components/ui/switch";
  import Busy from "$lib/components/busy.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";
  import { timeAgo, lastRunGlyph, lastRunClass, triggerSourceLabel } from "$lib/workflows/format";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].workflowsList);
  import TemplateGallery from "./TemplateGallery.svelte";
  import DictionaryTablesTab from "./DictionaryTablesTab.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import Play from "@lucide/svelte/icons/play";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Copy from "@lucide/svelte/icons/copy";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import Globe from "@lucide/svelte/icons/globe";
  import Layers from "@lucide/svelte/icons/layers";
  import History from "@lucide/svelte/icons/history";
  import Folder from "@lucide/svelte/icons/folder";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import Workflow from "@lucide/svelte/icons/workflow";
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import Table2 from "@lucide/svelte/icons/table-2";

  // --- domain types (verbatim copies of the parent's — kept structurally identical so the
  //     callback props below stay assignable from the parent's functions) ---
  type ScheduleFreq = "hourly" | "daily" | "weekly" | "monthly" | "yearly";
  type ScheduleT = {
    freq: ScheduleFreq;
    tz: string;
    times: string[];
    minute: number;
    weekdays: number[]; // 0=Mon … 6=Sun
    day_of_month: number;
    month: number; // 1-12
  };
  type Trigger = "Manual" | { Cron: { every_secs: number } } | { Schedule: ScheduleT };
  type Concurrency =
    | "SkipIfRunning"
    | { QueueSingle: { max_queued: number } }
    | { Concurrent: { max_concurrent: number } };
  type Node = { id: number; kind: string; x: number; y: number; config: Record<string, unknown>; memoize?: boolean };
  type Edge = { from: number; from_port: string; to: number; to_port: string };
  type VarDecl = {
    name: string;
    type: string;
    default?: string | null;
    param?: boolean;
    description?: string | null;
  };
  type Graph = {
    id: string;
    name: string;
    description?: string;
    group?: string;
    enabled: boolean;
    trigger: Trigger;
    concurrency: Concurrency;
    instance_scope?: "Single" | "PerCamera";
    run_orphans?: boolean;
    debug?: boolean;
    nodes: Node[];
    edges: Edge[];
    next_id?: number;
    viewport?: { x: number; y: number; zoom: number };
    edge_vias?: Record<string, [number, number][]>;
    variables?: VarDecl[];
    template?: boolean;
    global?: boolean;
    last_run?: { at: string; status: string; trigger: string } | null;
  };
  type Activity = {
    running: number;
    current: number | null;
    run_id?: string | null;
    trigger?: string | null;
    pod_name?: string | null;
  };
  type TemplateSummary = { id: string; name: string; description: string; global: boolean; inputs: { name: string; type: string }[] };
  type ListTab = "mine" | "templates" | "dictionaries";

  let {
    // data (owned by the parent)
    graphs,
    templates,
    templatesLoading,
    usingTemplateId,
    allGroups,
    collapsedGroups,
    activity,
    isMaster,
    wfSearch,
    wfGroupFilter,
    listTab = $bindable(),
    // list-level actions
    createWorkflow,
    showTemplatesTab,
    cloneTemplate,
    reloadDictionaries,
    toggleGroup,
    onSearchInput,
    setGroupFilter,
    clearListFilters,
    // row actions + presentation helpers
    openGraph,
    runNow,
    toggleEnabled,
    toggleTemplate,
    toggleGlobal,
    duplicateWorkflow,
    openRuns,
    openDelete,
    runStatus,
    dotState,
    triggerSummary,
    graphHasUnfilled,
    listRunTitle,
    listRunDisabled,
  }: {
    graphs: Graph[];
    templates: TemplateSummary[];
    templatesLoading: boolean;
    usingTemplateId: string | null;
    allGroups: string[];
    collapsedGroups: Set<string>;
    activity: Record<string, Activity>;
    isMaster: boolean;
    wfSearch: string;
    wfGroupFilter: string;
    listTab: ListTab;
    createWorkflow: () => void;
    showTemplatesTab: () => void;
    cloneTemplate: (t: TemplateSummary) => void;
    reloadDictionaries: () => void;
    toggleGroup: (key: string) => void;
    onSearchInput: (v: string) => void;
    setGroupFilter: (v: string) => void;
    clearListFilters: () => void;
    openGraph: (id: string) => void;
    runNow: (g: Graph) => void;
    toggleEnabled: (g: Graph, enabled: boolean) => void;
    toggleTemplate: (g: Graph) => void;
    toggleGlobal: (g: Graph) => void;
    duplicateWorkflow: (g: Graph) => void;
    openRuns: (g: Graph) => void;
    openDelete: (g: Graph) => void;
    runStatus: (g: Graph) => "running" | "idle" | "disabled";
    dotState: (g: Graph) => "online" | "warn" | "offline";
    triggerSummary: (t: Trigger) => string;
    graphHasUnfilled: (g: Graph) => boolean;
    listRunTitle: (g: Graph, unfilled: boolean) => string;
    listRunDisabled: (g: Graph, unfilled: boolean) => boolean;
  } = $props();

  // Workflows clustered by group (named groups first, "Ungrouped" last), like the logs grouping.
  const groupedGraphs = $derived.by(() => {
    const map = new Map<string, Graph[]>();
    for (const g of graphs) {
      const key = (g.group ?? "").trim();
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(g);
    }
    const keys = [...map.keys()].sort((a, b) => {
      if (a === "") return 1;
      if (b === "") return -1;
      return a.localeCompare(b);
    });
    return keys.map((k) => ({ key: k, label: k || "Ungrouped", rows: map.get(k)! }));
  });

  function listTabClass(t: ListTab): string {
    const base = "flex items-center gap-2 border-b-2 px-3 py-2 text-sm font-medium transition-colors -mb-px";
    return listTab === t
      ? `${base} border-primary text-foreground`
      : `${base} border-transparent text-muted-foreground hover:text-foreground`;
  }
</script>

{#snippet wfRow(g: Graph, i: number)}
  {@const st = runStatus(g)}
  {@const unfilled = graphHasUnfilled(g)}
  <div
    class="wf-row group flex cursor-pointer items-center gap-3 border-l-2 border-transparent py-1.5 pl-9 pr-3 transition-colors hover:border-primary/40 hover:bg-primary/5"
    style="animation-delay: {Math.min(i, 14) * 26}ms"
    role="button"
    tabindex="0"
    onclick={(e) => {
      if ((e.target as HTMLElement).closest("[data-stop]")) return;
      openGraph(g.id);
    }}
    onkeydown={(e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openGraph(g.id);
      }
    }}
  >
    <StatusDot state={dotState(g)} />
    <span class="min-w-0 flex-1 truncate text-sm">
      <span class="font-medium">{g.name}</span>
      {#if g.description}<span class="ml-1.5 text-xs text-muted-foreground">— {g.description}</span>{/if}
    </span>
    <span class="hidden w-32 shrink-0 truncate text-xs text-muted-foreground md:inline">{triggerSummary(g.trigger)}</span>
    <span class="hidden w-8 shrink-0 text-right text-xs tabular-nums text-muted-foreground sm:inline">{g.nodes.length}</span>
    <span class="w-24 shrink-0 text-xs">
      {#if st === "running"}
        <span class="inline-flex items-center gap-1 font-medium text-primary">
          <Busy size={11} />
          {#if activity[g.id]?.trigger}{triggerSourceLabel(activity[g.id]?.trigger, t)}{:else}{t.running}{/if}
        </span>
      {:else if g.last_run}
        <span class="flex items-center gap-1 {lastRunClass(g.last_run.status)}">
          {lastRunGlyph(g.last_run.status)} {timeAgo(g.last_run.at, t)}
        </span>
      {:else}
        <span class="text-muted-foreground/50">—</span>
      {/if}
    </span>
    <div data-stop>
      <Switch checked={g.enabled} onCheckedChange={(v) => toggleEnabled(g, v)} />
    </div>
    <div class="flex items-center gap-0.5" data-stop>
      <Button
        variant="ghost"
        size="icon"
        class="size-7 text-primary"
        title={listRunTitle(g, unfilled)}
        disabled={listRunDisabled(g, unfilled)}
        onclick={() => runNow(g)}
      >
        {#if st === "running"}<Busy />{:else}<Play class="size-3.5" />{/if}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        class="size-7 {g.template ? 'text-primary' : ''}"
        title={g.template ? t.unmakeTemplate : t.makeTemplate}
        onclick={() => toggleTemplate(g)}
      >
        <Bookmark class="size-3.5" fill={g.template ? "currentColor" : "none"} />
      </Button>
      {#if isMaster && g.template}
        <Button
          variant="ghost"
          size="icon"
          class="size-7 {g.global ? 'text-primary' : ''}"
          title={g.global ? t.unpublishGlobal : t.publishGlobal}
          onclick={() => toggleGlobal(g)}
        >
          <Globe class="size-3.5" />
        </Button>
      {/if}
      <Button variant="ghost" size="icon" class="size-7" title={t.duplicate} onclick={() => duplicateWorkflow(g)}>
        <Copy class="size-3.5" />
      </Button>
      <Button variant="ghost" size="icon" class="size-7" title={t.liveInstances} onclick={() => goto(`/workflows/instances?graph=${g.id}&name=${encodeURIComponent(g.name)}`)}>
        <Layers class="size-3.5" />
      </Button>
      <Button variant="ghost" size="icon" class="size-7" title={t.recentRuns} onclick={() => openRuns(g)}>
        <History class="size-3.5" />
      </Button>
      <Button variant="ghost" size="icon" class="size-7" title={t.delete} onclick={() => openDelete(g)}>
        <Trash2 class="size-3.5" />
      </Button>
    </div>
  </div>
{/snippet}

<div class="flex h-full flex-col gap-4 overflow-hidden p-6">
  <!-- header -->
  <div class="flex shrink-0 items-center justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{t.title}</h1>
      <p class="text-sm text-muted-foreground">{t.description}</p>
    </div>
    <div class="flex items-center gap-2">
      <Button onclick={createWorkflow}><Plus class="size-4" /> {t.newWorkflow}</Button>
    </div>
  </div>

  <!-- tabs: my workflows | templates | dictionary tables — all inline (#136, #137) -->
  <div class="flex shrink-0 items-center gap-1 border-b border-border">
    <button type="button" class={listTabClass("mine")} onclick={() => (listTab = "mine")}>
      <Workflow class="size-4" /> {t.tabMine}
      {#if graphs.length}<span class="rounded-full bg-muted px-1.5 text-xs text-muted-foreground">{graphs.length}</span>{/if}
    </button>
    <button type="button" class={listTabClass("templates")} onclick={showTemplatesTab}>
      <LayoutTemplate class="size-4" /> {t.tabTemplates}
      {#if templates.length}<span class="rounded-full bg-muted px-1.5 text-xs text-muted-foreground">{templates.length}</span>{/if}
    </button>
    <button type="button" class={listTabClass("dictionaries")} onclick={() => (listTab = "dictionaries")}>
      <Table2 class="size-4" /> {t.tabDictionaries}
    </button>
  </div>

  {#if listTab === "mine"}
  <!-- filter bar -->
  <div class="flex shrink-0 flex-wrap items-center gap-2">
    <div class="relative">
      <Search
        class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        value={wfSearch}
        oninput={(e) => onSearchInput(e.currentTarget.value)}
        placeholder={t.searchPlaceholder}
        class="h-9 w-64 pl-8"
      />
    </div>
    <select
      value={wfGroupFilter}
      onchange={(e) => setGroupFilter(e.currentTarget.value)}
      class="h-9 rounded-lg border border-border bg-card px-2.5 text-sm text-foreground outline-none"
    >
      <option value="">{t.allGroups}</option>
      {#each allGroups as grp (grp)}<option value={grp}>{grp}</option>{/each}
    </select>
    {#if wfSearch.trim() || wfGroupFilter}
      <Button variant="ghost" size="sm" class="h-9" onclick={clearListFilters}>
        <X class="size-4" /> {t.clear}
      </Button>
    {/if}
  </div>

  <!-- table -->
  <div class="min-h-0 flex-1 overflow-y-auto rounded-xl border border-border">
    {#if graphs.length === 0}
      {#if wfSearch.trim() || wfGroupFilter}
        <div class="flex flex-col items-center gap-3 p-12 text-center text-sm text-muted-foreground">
          {t.noMatches}
          <Button variant="outline" size="sm" onclick={clearListFilters}>{t.clearFilters}</Button>
        </div>
      {:else}
        <div class="p-6">
          <EmptyState
            title={t.emptyTitle}
            description={t.emptyDescription}
          >
            <div class="flex flex-wrap items-center justify-center gap-2">
              <Button onclick={showTemplatesTab}><LayoutTemplate class="size-4" /> {t.browseTemplates}</Button>
              <Button variant="outline" onclick={createWorkflow}><Plus class="size-4" /> {t.newWorkflow}</Button>
            </div>
          </EmptyState>
        </div>
      {/if}
    {:else}
      <div class="divide-y divide-border/60">
        {#each groupedGraphs as grp, gi (grp.key)}
          {@const collapsed = collapsedGroups.has(grp.key)}
          {@const base = groupedGraphs.slice(0, gi).reduce((n, x) => n + x.rows.length, 0)}
          <div>
            <!-- category root -->
            <button
              type="button"
              class="wf-group sticky top-0 z-10 flex w-full items-center gap-2 bg-card px-3 py-2 text-left text-sm font-semibold transition-colors hover:bg-hover"
              style="animation-delay: {Math.min(base, 14) * 26}ms"
              onclick={() => toggleGroup(grp.key)}
            >
              <ChevronRight
                class="size-4 shrink-0 text-muted-foreground transition-transform {collapsed ? '' : 'rotate-90'}"
              />
              <Folder class="size-4 shrink-0 text-primary/70" />
              <span class="truncate">{grp.label}</span>
              <span class="text-xs font-normal text-muted-foreground/60">{grp.rows.length}</span>
            </button>
            {#if !collapsed}
              <div class="divide-y divide-border/40">
                {#each grp.rows as g, i (g.id)}
                  {@render wfRow(g, base + i)}
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
  {:else if listTab === "templates"}
    <TemplateGallery
      {templates}
      loading={templatesLoading}
      usingId={usingTemplateId}
      onUse={cloneTemplate}
      onBack={() => (listTab = "mine")}
    />
  {:else}
    <DictionaryTablesTab onChanged={reloadDictionaries} />
  {/if}
</div>

<style>
  /* Workflow list rows: a staggered fade + slide-in on first paint (impeccable entrance —
     arrival that reads as the list settling into place). Keyed rows only animate when they
     actually mount, so filtering/mutating doesn't re-run it. The per-row delay is set inline. */
  @keyframes wf-row-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  :global(.wf-row) {
    animation: wf-row-in 260ms cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  /* Group headers lead their block in. */
  :global(.wf-group) {
    animation: wf-row-in 220ms cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.wf-row),
    :global(.wf-group) {
      animation: none;
    }
  }
</style>
