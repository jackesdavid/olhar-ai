<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import ScrollText from "@lucide/svelte/icons/scroll-text";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Copy from "@lucide/svelte/icons/copy";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import * as Table from "$lib/components/ui/table";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import { platformLogs, type PlatformLog, type PlatformLogsFilter } from "$lib/api";
  import { nodeKindColor } from "$lib/workflows/portColors";

  // ---- state ------------------------------------------------------------------
  let loading = $state(true);
  let loadingMore = $state(false);
  let items = $state<PlatformLog[]>([]);
  let nextCursor = $state<number | null>(null);

  // filters (bound to URL params for deep-linking)
  let filterModule     = $state<string>("");
  let filterTraceId    = $state<string>("");
  let filterEntityType = $state<string>("");
  let filterEntityId   = $state<string>("");
  let filterLevel      = $state<string>("");
  let filterFrom       = $state<string>("");
  let filterTo         = $state<string>("");
  let filterSearch     = $state<string>("");

  const MODULES   = ["", "workflow", "edge", "presence", "device", "alert"];
  const LEVELS    = ["", "debug", "info", "warn", "error"];
  const DATE_PRESETS = [
    { label: "Last hour",  hours: 1 },
    { label: "6 h",        hours: 6 },
    { label: "24 h",       hours: 24 },
    { label: "7 days",     hours: 24 * 7 },
    { label: "30 days",    hours: 24 * 30 },
  ];

  // ---- sort -------------------------------------------------------------------
  type SortCol = "created_at" | "module" | "level";
  let sortCol = $state<SortCol>("created_at");
  let sortDir = $state<"asc" | "desc">("desc");

  function toggleSort(col: SortCol) {
    if (sortCol === col) {
      sortDir = sortDir === "asc" ? "desc" : "asc";
    } else {
      sortCol = col;
      sortDir = col === "created_at" ? "desc" : "asc";
    }
  }

  // ---- derived ----------------------------------------------------------------
  const filtered = $derived(
    filterSearch.trim()
      ? items.filter(l => l.message.toLowerCase().includes(filterSearch.trim().toLowerCase()))
      : items,
  );

  const shown = $derived(
    [...filtered].sort((a, b) => {
      const av = a[sortCol] as string;
      const bv = b[sortCol] as string;
      const cmp = av.localeCompare(bv);
      return sortDir === "asc" ? cmp : -cmp;
    }),
  );

  // ---- URL sync ---------------------------------------------------------------
  function filtersToParams(): URLSearchParams {
    const p = new URLSearchParams();
    if (filterModule)     p.set("module",      filterModule);
    if (filterTraceId)    p.set("trace_id",    filterTraceId);
    if (filterEntityType) p.set("entity_type", filterEntityType);
    if (filterEntityId)   p.set("entity_id",   filterEntityId);
    if (filterLevel)      p.set("level",       filterLevel);
    if (filterFrom)       p.set("from",        filterFrom);
    if (filterTo)         p.set("to",          filterTo);
    return p;
  }

  function pushUrl() {
    const p = filtersToParams();
    const qs = p.toString();
    goto(`/logs${qs ? `?${qs}` : ""}`, { replaceState: true, noScroll: true, keepFocus: true });
  }

  function readUrl() {
    const p = $page.url.searchParams;
    filterModule     = p.get("module")      ?? "";
    filterTraceId    = p.get("trace_id")    ?? "";
    filterEntityType = p.get("entity_type") ?? "";
    filterEntityId   = p.get("entity_id")   ?? "";
    filterLevel      = p.get("level")       ?? "";
    filterFrom       = p.get("from")        ?? "";
    filterTo         = p.get("to")          ?? "";
  }

  // ---- fetch ------------------------------------------------------------------
  function buildFilter(after?: number): PlatformLogsFilter {
    const f: PlatformLogsFilter = { limit: 50 };
    if (filterModule)     f.module      = filterModule;
    if (filterTraceId)    f.trace_id    = filterTraceId;
    if (filterEntityType) f.entity_type = filterEntityType;
    if (filterEntityId)   f.entity_id   = filterEntityId;
    if (filterLevel)      f.level       = filterLevel;
    if (filterFrom)       f.from        = filterFrom;
    if (filterTo)         f.to          = filterTo;
    if (after != null)    f.after       = after;
    return f;
  }

  async function load() {
    loading = true;
    items = [];
    nextCursor = null;
    try {
      const r = await platformLogs.list(buildFilter());
      items = r.items;
      nextCursor = r.next_cursor;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load logs");
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    if (!nextCursor || loadingMore) return;
    loadingMore = true;
    try {
      const r = await platformLogs.list(buildFilter(nextCursor));
      items = [...items, ...r.items];
      nextCursor = r.next_cursor;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load more logs");
    } finally {
      loadingMore = false;
    }
  }

  async function applyFilters() {
    pushUrl();
    await load();
  }

  // ---- preset dates -----------------------------------------------------------
  function applyPreset(hours: number) {
    const now = new Date();
    const from = new Date(now.getTime() - hours * 3_600_000);
    filterFrom = from.toISOString().slice(0, 19) + "Z";
    filterTo = now.toISOString().slice(0, 19) + "Z";
  }

  // ---- helpers ----------------------------------------------------------------
  function fmtTs(ts: string): string {
    return ts.slice(0, 19).replace("T", " ");
  }

  function levelColor(lvl: string): string {
    if (lvl === "error") return "text-destructive";
    if (lvl === "warn")  return "text-amber-500";
    if (lvl === "debug") return "text-muted-foreground/60";
    return "text-foreground/80";
  }

  function moduleColor(mod: string): string {
    const map: Record<string, string> = {
      workflow: "bg-primary/15 text-primary",
      edge:     "bg-blue-500/15 text-blue-400",
      presence: "bg-muted text-foreground",
      device:   "bg-purple-500/15 text-purple-400",
      alert:    "bg-amber-500/15 text-amber-400",
    };
    return map[mod] ?? "bg-muted text-muted-foreground";
  }

  const MODULE_DOT: Record<string, string> = {
    workflow: "#e5e5e5",
    edge:     "#60a5fa",
    presence: "#34d399",
    device:   "#a78bfa",
    alert:    "#fbbf24",
  };

  function logDotColor(l: PlatformLog): string {
    const nk = (l.payload as Record<string, unknown> | null)?.node_kind as string | undefined;
    if (l.module === "workflow" && nk) return nodeKindColor(nk);
    return MODULE_DOT[l.module] ?? "#64748b";
  }

  function setTraceFilter(traceId: string) {
    filterTraceId = traceId;
    applyFilters();
  }

  function setEntityFilter(entityType: string | null, entityId: string | null) {
    filterEntityType = entityType ?? "";
    filterEntityId   = entityId   ?? "";
    applyFilters();
  }

  function copyText(text: string) {
    navigator.clipboard.writeText(text).catch(() => {});
    toast.success("Copied");
  }

  // ---- mount ------------------------------------------------------------------
  onMount(() => {
    readUrl();
    load();
  });
</script>

<div class="flex h-full flex-col gap-3 p-4">
  <!-- header -->
  <div class="flex flex-wrap items-center gap-3">
    <div class="flex items-center gap-2">
      <ScrollText class="size-5 text-primary" />
      <h1 class="text-xl font-semibold tracking-tight">Platform Logs</h1>
    </div>
    <p class="text-sm text-muted-foreground">
      Unified audit log for all modules — workflows, edge boxes, presence, devices.
    </p>
  </div>

  <!-- filter panel -->
  <div class="flex flex-col gap-2 rounded-lg border border-border bg-card/40 p-3">
    <div class="flex flex-wrap items-center gap-2">
      <!-- Module -->
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Module</Label>
        <select
          class="h-8 rounded-md border border-border bg-background px-2 text-sm"
          bind:value={filterModule}
        >
          <option value="">All</option>
          {#each MODULES.slice(1) as m (m)}<option value={m}>{m}</option>{/each}
        </select>
      </div>
      <!-- Level -->
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Level</Label>
        <select
          class="h-8 rounded-md border border-border bg-background px-2 text-sm"
          bind:value={filterLevel}
        >
          <option value="">All</option>
          {#each LEVELS.slice(1) as l (l)}<option value={l}>{l}</option>{/each}
        </select>
      </div>
      <!-- Trace ID -->
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Trace ID</Label>
        <div class="relative">
          <Input
            bind:value={filterTraceId}
            placeholder="UUID…"
            class="h-8 w-72 font-mono text-xs"
          />
          {#if filterTraceId}
            <button
              class="interactive absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              onclick={() => { filterTraceId = ""; }}
            ><X class="size-3" /></button>
          {/if}
        </div>
      </div>
      <!-- Entity -->
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Entity</Label>
        <Input
          bind:value={filterEntityId}
          placeholder="ID…"
          class="h-8 w-28 font-mono text-xs"
        />
        <select
          class="h-8 rounded-md border border-border bg-background px-2 text-sm"
          bind:value={filterEntityType}
        >
          <option value="">Any type</option>
          <option value="workflow_graph">workflow_graph</option>
          <option value="edge_box">edge_box</option>
          <option value="device">device</option>
        </select>
      </div>
    </div>

    <!-- date range row -->
    <div class="flex flex-wrap items-center gap-2">
      <Label class="text-xs text-muted-foreground">Date range</Label>
      <div class="flex items-center gap-1">
        {#each DATE_PRESETS as p (p.label)}
          <Button
            variant="outline"
            size="sm"
            class="h-7 px-2 text-xs"
            onclick={() => applyPreset(p.hours)}
          >{p.label}</Button>
        {/each}
      </div>
      <Input bind:value={filterFrom} placeholder="From (RFC3339)" class="h-8 w-52 font-mono text-xs" />
      <span class="text-xs text-muted-foreground">→</span>
      <Input bind:value={filterTo} placeholder="To (RFC3339)" class="h-8 w-52 font-mono text-xs" />
      {#if filterFrom || filterTo}
        <button
          class="interactive text-xs text-muted-foreground hover:text-foreground"
          onclick={() => { filterFrom = ""; filterTo = ""; }}
        ><X class="size-3.5" /></button>
      {/if}
    </div>

    <!-- search + apply -->
    <div class="flex flex-wrap items-center gap-2">
      <div class="relative min-w-48 flex-1">
        <Search class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input bind:value={filterSearch} placeholder="Filter messages (client-side)…" class="h-8 pl-8" />
        {#if filterSearch}
          <button
            class="interactive absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            onclick={() => (filterSearch = "")}
          ><X class="size-3.5" /></button>
        {/if}
      </div>
      <Button size="sm" class="h-8" onclick={applyFilters}>Apply filters</Button>
      <Button
        variant="outline"
        size="sm"
        class="h-8"
        onclick={() => {
          filterModule = ""; filterTraceId = ""; filterEntityType = "";
          filterEntityId = ""; filterLevel = ""; filterFrom = ""; filterTo = ""; filterSearch = "";
          applyFilters();
        }}
      >Clear all</Button>
      {#if !loading}
        <span class="text-xs tabular-nums text-muted-foreground">{shown.length} shown</span>
      {/if}
    </div>
  </div>

  <!-- log table -->
  {#if loading}
    <Loader label="Loading logs…" />
  {:else if shown.length === 0}
    <EmptyState
      title="No logs found"
      description="Adjust the filters or wait for activity."
    />
  {:else}
    <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border">
      <div class="min-h-0 flex-1 overflow-auto">
        <Table.Root class="w-full">
          <Table.Header class="sticky top-0 z-10 bg-card">
            <Table.Row>
              <Table.Head class="w-40">
                <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("created_at")}>
                  Time
                  {#if sortCol === "created_at"}<span class="text-primary">{sortDir === "asc" ? "↑" : "↓"}</span>{:else}<span class="text-muted-foreground/40">↕</span>{/if}
                </button>
              </Table.Head>
              <Table.Head class="w-5"></Table.Head>
              <Table.Head class="w-24">
                <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("module")}>
                  Module
                  {#if sortCol === "module"}<span class="text-primary">{sortDir === "asc" ? "↑" : "↓"}</span>{:else}<span class="text-muted-foreground/40">↕</span>{/if}
                </button>
              </Table.Head>
              <Table.Head class="w-14">
                <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("level")}>
                  Level
                  {#if sortCol === "level"}<span class="text-primary">{sortDir === "asc" ? "↑" : "↓"}</span>{:else}<span class="text-muted-foreground/40">↕</span>{/if}
                </button>
              </Table.Head>
              <Table.Head class="w-28">Entity</Table.Head>
              <Table.Head>Message</Table.Head>
              <Table.Head class="w-32">Trace ID</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each shown as l (l.id)}
              <Table.Row class="align-top">
                <Table.Cell class="whitespace-nowrap font-mono text-[11px] tabular-nums text-muted-foreground">
                  {fmtTs(l.created_at)}
                </Table.Cell>
                <Table.Cell class="pr-0">
                  <span
                    class="inline-block size-2 rounded-full"
                    style:background-color={logDotColor(l)}
                    title={((l.payload as Record<string, unknown> | null)?.node_kind as string) ?? l.module}
                  ></span>
                </Table.Cell>
                <Table.Cell>
                  <span class="inline-flex items-center rounded px-1.5 py-0.5 font-mono text-[10px] font-medium {moduleColor(l.module)}">
                    {l.module}
                  </span>
                </Table.Cell>
                <Table.Cell class="font-mono text-[11px] uppercase {levelColor(l.level)}">{l.level}</Table.Cell>
                <Table.Cell class="font-mono text-[10px] text-muted-foreground">
                  {#if l.entity_id}
                    <button
                      class="interactive truncate hover:text-foreground"
                      title="{l.entity_type}: {l.entity_id}"
                      onclick={() => setEntityFilter(l.entity_type, l.entity_id)}
                    >
                      {l.entity_id.length > 8 ? l.entity_id.slice(0, 8) + "…" : l.entity_id}
                    </button>
                  {/if}
                </Table.Cell>
                <Table.Cell class="font-mono text-[11px] {levelColor(l.level)}">
                  <span class="whitespace-pre-wrap break-words">{l.message}</span>
                  {#if l.payload}
                    <details class="mt-0.5">
                      <summary class="cursor-pointer text-[10px] text-muted-foreground hover:text-foreground">payload</summary>
                      <pre class="mt-1 overflow-x-auto rounded bg-well p-1 text-[10px] text-muted-foreground">{JSON.stringify(l.payload, null, 2)}</pre>
                    </details>
                  {/if}
                </Table.Cell>
                <Table.Cell class="font-mono text-[10px] text-muted-foreground">
                  {#if l.trace_id}
                    <div class="flex items-center gap-1">
                      <button
                        class="interactive truncate hover:text-foreground"
                        title="Filter by trace ID: {l.trace_id}"
                        onclick={() => setTraceFilter(l.trace_id!)}
                      >{l.trace_id.slice(0, 8)}…</button>
                      <button
                        class="interactive shrink-0 text-muted-foreground/60 hover:text-foreground"
                        title="Copy trace ID"
                        onclick={() => copyText(l.trace_id!)}
                      ><Copy class="size-3" /></button>
                    </div>
                  {/if}
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      </div>

      <!-- pagination footer -->
      <div class="flex items-center justify-between border-t border-border bg-card/40 px-3 py-1.5 text-xs text-muted-foreground">
        <span class="tabular-nums">{shown.length} entries{nextCursor ? " · more available" : ""}</span>
        {#if nextCursor}
          <Button
            variant="outline"
            size="sm"
            class="h-7"
            disabled={loadingMore}
            onclick={loadMore}
          >{loadingMore ? "Loading…" : "Load more"}</Button>
        {/if}
      </div>
    </div>
  {/if}
</div>
