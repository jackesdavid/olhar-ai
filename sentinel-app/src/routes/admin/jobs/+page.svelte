<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Activity from "@lucide/svelte/icons/activity";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Select from "$lib/components/ui/select";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";

  import { adminJobs, admin, type CategoryStat, type MonitorJob, type Tenant, type JobSeries } from "$lib/api";
  import { auth } from "$lib/auth.svelte";
  import { fmtTs } from "$lib/utils";

  const isMaster = auth.me?.is_master ?? false;
  const PAGE_SIZE = 50;

  let loading = $state(true);
  let stats = $state<CategoryStat[]>([]);
  let items = $state<MonitorJob[]>([]);
  let total = $state(0);
  let tenants = $state<Tenant[]>([]);
  let series = $state<JobSeries | null>(null);

  let tenantFilter = $state(""); // "" = all tenants
  let categoryFilter = $state("");
  let page = $state(0);

  // Time window — the panel counts only within it (recent past + live, not all history).
  const WINDOW_OPTIONS = [
    { value: "60", label: "Last minute" },
    { value: "3600", label: "Last hour" },
    { value: "86400", label: "Last day" },
    { value: "259200", label: "Last 3 days" },
    { value: "604800", label: "Last week" },
  ];
  let windowSecs = $state(86400); // default: last day
  function windowLabel(secs: number): string {
    return WINDOW_OPTIONS.find((o) => o.value === String(secs))?.label ?? "Last day";
  }

  // Auto-refresh: 0 = off. A silent reload each tick (no top-level spinner) so the panel updates
  // in place without flicker.
  const REFRESH_OPTIONS = [
    { value: "0", label: "Off" },
    { value: "5", label: "5s" },
    { value: "10", label: "10s" },
    { value: "20", label: "20s" },
    { value: "60", label: "1 min" },
  ];
  let autoSecs = $state(0);
  function autoLabel(secs: number): string {
    return REFRESH_OPTIONS.find((o) => o.value === String(secs))?.label ?? "Off";
  }

  const pageCount = $derived(Math.max(1, Math.ceil(total / PAGE_SIZE)));

  // Distinct categories present, for the KPI cards + the filter.
  const categories = $derived([...new Set(stats.map((s) => s.category))].sort());
  // A quick lookup: count for (category, status).
  function count(category: string, status: string): number {
    return stats.find((s) => s.category === category && s.status === status)?.count ?? 0;
  }
  function categoryTotal(category: string): number {
    return stats.filter((s) => s.category === category).reduce((a, s) => a + s.count, 0);
  }
  // System-wide open work (pending + running) — the "is anything backing up?" number.
  const openNow = $derived(stats.filter((s) => s.status === "pending" || s.status === "running").reduce((a, s) => a + s.count, 0));
  const failedTotal = $derived(stats.filter((s) => s.status === "failed").reduce((a, s) => a + s.count, 0));

  function tenantName(id: string): string {
    return tenants.find((t) => t.id === id)?.name ?? id.slice(0, 8);
  }
  function prettyCategory(c: string): string {
    if (c === "clip_delete") return "Clip delete";
    if (c === "workflow_event") return "Workflow events";
    if (c === "notification") return "Notifications";
    return c;
  }
  function statusClass(status: string): string {
    if (status === "failed") return "border-destructive/30 bg-destructive/10 text-destructive";
    if (status === "running") return "border-primary/25 bg-primary/10 text-primary animate-pulse";
    if (status === "pending") return "border-white/15 bg-muted/40 text-muted-foreground";
    return "border-white/15 bg-muted/40 text-muted-foreground"; // done
  }

  async function loadStats() {
    stats = await adminJobs.stats(tenantFilter || undefined, windowSecs);
  }
  async function loadSeries() {
    series = await adminJobs.series(tenantFilter || undefined, windowSecs);
  }

  // Zero-filled bars from the sparse series: the backend returns only non-empty buckets, so build
  // the full axis (now-window → now, stepping bucket_secs) and look counts up. ~120 buckets max.
  const chartBars = $derived.by(() => {
    if (!series || series.bucket_secs <= 0) return [] as { t: number; c: number }[];
    const bucket = series.bucket_secs;
    const map = new Map(series.points.map((p) => [p.t, p.c]));
    const now = Math.floor(Date.now() / 1000);
    const start = Math.floor((now - windowSecs) / bucket) * bucket;
    const end = Math.floor(now / bucket) * bucket;
    const bars: { t: number; c: number }[] = [];
    for (let t = start; t <= end; t += bucket) bars.push({ t, c: map.get(t) ?? 0 });
    return bars;
  });
  const maxBar = $derived(Math.max(1, ...chartBars.map((b) => b.c)));
  const chartEmpty = $derived(chartBars.length === 0 || chartBars.every((b) => b.c === 0));
  function barTitle(b: { t: number; c: number }): string {
    const d = new Date(b.t * 1000);
    return `${d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })} · ${b.c}`;
  }
  function axisLabel(t: number): string {
    // Show seconds on the short (sub-5-min) view, where minute-only labels would all read the same.
    const opts: Intl.DateTimeFormatOptions =
      windowSecs <= 300 ? { minute: "2-digit", second: "2-digit" } : { hour: "2-digit", minute: "2-digit" };
    return new Date(t * 1000).toLocaleTimeString([], opts);
  }
  async function loadList() {
    const res = await adminJobs.list({
      tenant: tenantFilter || undefined,
      category: categoryFilter || undefined,
      window: windowSecs,
      limit: PAGE_SIZE,
      offset: page * PAGE_SIZE,
    });
    items = res.items;
    total = res.total;
  }
  async function refresh() {
    loading = true;
    try {
      await Promise.all([loadStats(), loadList(), loadSeries()]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load jobs");
    } finally {
      loading = false;
    }
  }
  // A quiet reload for the auto-refresh tick: no spinner, and a transient error keeps the last
  // good data on screen instead of surfacing a toast every few seconds.
  async function silentRefresh() {
    try {
      await Promise.all([loadStats(), loadList(), loadSeries()]);
    } catch { /* keep last good data */ }
  }

  // Reinstalls the interval whenever the cadence changes; cleared on change/unmount.
  $effect(() => {
    if (autoSecs <= 0) return;
    const t = setInterval(silentRefresh, autoSecs * 1000);
    return () => clearInterval(t);
  });

  function onTenantChange(v: string) {
    tenantFilter = v === "_all" ? "" : v;
    page = 0;
    refresh();
  }
  function onCategoryChange(v: string) {
    categoryFilter = v === "_all" ? "" : v;
    page = 0;
    loadList();
  }
  function onWindowChange(v: string) {
    windowSecs = Number(v);
    page = 0;
    refresh();
  }
  function prevPage() { if (page > 0) { page--; loadList(); } }
  function nextPage() { if (page < pageCount - 1) { page++; loadList(); } }

  onMount(async () => {
    if (!isMaster) { loading = false; return; }
    try {
      tenants = await admin.tenants();
    } catch { /* non-fatal — the selector just shows ids */ }
    await refresh();
  });
</script>

<div class="flex flex-col gap-6 p-6">
  <PageHeader
    title="System load"
    description="Every async job across the platform — clip deletion, workflow events, notification sends — in one queue."
    icon={Activity}
  >
    {#snippet actions()}
      {#if isMaster}
        <div class="flex items-center gap-2">
          <Select.Root type="single" value={String(windowSecs)} onValueChange={onWindowChange}>
            <Select.Trigger class="h-8 w-32 text-sm" title="Time window">
              {windowLabel(windowSecs)}
            </Select.Trigger>
            <Select.Content>
              {#each WINDOW_OPTIONS as opt (opt.value)}
                <Select.Item value={opt.value} label={opt.label}>{opt.label}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
          <Select.Root type="single" value={tenantFilter || "_all"} onValueChange={onTenantChange}>
            <Select.Trigger class="h-8 w-48 text-sm">
              {tenantFilter ? tenantName(tenantFilter) : "All tenants"}
            </Select.Trigger>
            <Select.Content>
              <Select.Item value="_all" label="All tenants">All tenants</Select.Item>
              {#each tenants as t (t.id)}
                <Select.Item value={t.id} label={t.name}>{t.name}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
          <Select.Root type="single" value={String(autoSecs)} onValueChange={(v) => (autoSecs = Number(v))}>
            <Select.Trigger class="h-8 w-32 text-sm" title="Auto-refresh">
              Auto: {autoLabel(autoSecs)}
            </Select.Trigger>
            <Select.Content>
              {#each REFRESH_OPTIONS as opt (opt.value)}
                <Select.Item value={opt.value} label={opt.label}>{opt.label}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
          <Button variant="outline" size="sm" onclick={refresh} disabled={loading}>
            {#if loading}<Busy class="mr-2" />{:else}<RefreshCw class="mr-2 h-4 w-4" />{/if}
            Refresh
          </Button>
        </div>
      {/if}
    {/snippet}
  </PageHeader>

  {#if !isMaster}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      This console is for the SaaS administrator only.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <!-- headline numbers -->
    <div class="grid gap-4 sm:grid-cols-3">
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Open now (pending + running)</p>
        <p class="text-2xl font-semibold tabular-nums">{openNow}</p>
      </Card.Content></Card.Root>
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Failed</p>
        <p class="text-2xl font-semibold tabular-nums">{failedTotal}</p>
      </Card.Content></Card.Root>
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Categories</p>
        <p class="text-2xl font-semibold tabular-nums">{categories.length}</p>
      </Card.Content></Card.Root>
    </div>

    <!-- jobs over time -->
    <Card.Root>
      <Card.Content class="pt-6">
        <p class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Jobs over time · {windowLabel(windowSecs)}</p>
        {#if chartEmpty}
          <p class="mt-4 text-sm text-muted-foreground">No jobs in this window.</p>
        {:else}
          <div class="mt-4 flex h-28 items-end gap-px">
            {#each chartBars as b (b.t)}
              <div
                class="flex-1 rounded-sm bg-primary/70 transition-[height] hover:bg-primary"
                style="height: {Math.max(b.c > 0 ? 3 : 0, (b.c / maxBar) * 100)}%"
                title={barTitle(b)}
              ></div>
            {/each}
          </div>
          <div class="mt-1 flex justify-between text-[10px] text-muted-foreground tabular-nums">
            <span>{axisLabel(chartBars[0].t)}</span>
            <span>{axisLabel(chartBars[Math.floor(chartBars.length / 2)].t)}</span>
            <span>{axisLabel(chartBars[chartBars.length - 1].t)}</span>
          </div>
        {/if}
      </Card.Content>
    </Card.Root>

    <!-- per-category breakdown -->
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each categories as cat (cat)}
        <Card.Root>
          <Card.Content class="pt-6">
            <div class="flex items-baseline justify-between">
              <p class="text-sm font-semibold">{prettyCategory(cat)}</p>
              <span class="text-xs text-muted-foreground tabular-nums">{categoryTotal(cat)} total</span>
            </div>
            <div class="mt-3 flex flex-wrap gap-1.5">
              {#each ["pending", "running", "done", "failed"] as st (st)}
                {#if count(cat, st) > 0}
                  <Badge variant="outline" class={statusClass(st)}>{st} {count(cat, st)}</Badge>
                {/if}
              {/each}
            </div>
          </Card.Content>
        </Card.Root>
      {/each}
    </div>

    <!-- recent jobs -->
    <div class="flex items-center gap-2">
      <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Recent jobs</span>
      <Select.Root type="single" value={categoryFilter || "_all"} onValueChange={onCategoryChange}>
        <Select.Trigger class="h-8 w-44 text-sm">
          {categoryFilter ? prettyCategory(categoryFilter) : "All categories"}
        </Select.Trigger>
        <Select.Content>
          <Select.Item value="_all" label="All categories">All categories</Select.Item>
          {#each categories as cat (cat)}
            <Select.Item value={cat} label={prettyCategory(cat)}>{prettyCategory(cat)}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    {#if items.length === 0}
      <EmptyState title="No jobs" description="Nothing matches the current filter." />
    {:else}
      <div class="rounded-lg border border-white/10 overflow-hidden">
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Category</Table.Head>
              <Table.Head>Kind</Table.Head>
              <Table.Head>Status</Table.Head>
              {#if !tenantFilter}<Table.Head>Tenant</Table.Head>{/if}
              <Table.Head class="text-right">Attempts</Table.Head>
              <Table.Head>Created</Table.Head>
              <Table.Head>Error</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each items as j (j.id)}
              <Table.Row>
                <Table.Cell class="text-sm">{prettyCategory(j.category)}</Table.Cell>
                <Table.Cell class="text-sm text-muted-foreground">{j.kind}</Table.Cell>
                <Table.Cell><Badge variant="outline" class={statusClass(j.status)}>{j.status}</Badge></Table.Cell>
                {#if !tenantFilter}<Table.Cell class="text-sm">{tenantName(j.tenant_id)}</Table.Cell>{/if}
                <Table.Cell class="text-right tabular-nums">{j.attempts}</Table.Cell>
                <Table.Cell class="text-sm text-muted-foreground whitespace-nowrap">{fmtTs(j.created_at)}</Table.Cell>
                <Table.Cell class="max-w-xs truncate text-xs text-destructive">{j.last_error ?? ""}</Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      </div>

      {#if pageCount > 1}
        <div class="flex items-center justify-between text-sm text-muted-foreground">
          <span>Page {page + 1} of {pageCount} ({total} jobs)</span>
          <div class="flex gap-2">
            <Button variant="outline" size="sm" onclick={prevPage} disabled={page === 0}>Prev</Button>
            <Button variant="outline" size="sm" onclick={nextPage} disabled={page >= pageCount - 1}>Next</Button>
          </div>
        </div>
      {/if}
    {/if}
  {/if}
</div>
