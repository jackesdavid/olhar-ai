<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Gauge from "@lucide/svelte/icons/gauge";
  import { onMount } from "svelte";
  import { notifications, type QueueStats, type QueueItem } from "$lib/api";
  import { fmtTs } from "$lib/utils";
  import * as Table from "$lib/components/ui/table";
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Clock from "@lucide/svelte/icons/clock";
  import Send from "@lucide/svelte/icons/send";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Layers from "@lucide/svelte/icons/layers";
  import Search from "@lucide/svelte/icons/search";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";

  const PAGE_SIZE = 50;

  let stats = $state<QueueStats | null>(null);
  let items = $state<QueueItem[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let search = $state("");
  let statusFilter = $state("");
  let page = $state(0);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  const pageCount = $derived(Math.max(1, Math.ceil(total / PAGE_SIZE)));
  const kpis = $derived([
    { title: "Pending", value: stats?.pending ?? 0, icon: Clock },
    { title: "Sent today", value: stats?.sent_today ?? 0, icon: Send },
    { title: "Failed today", value: stats?.failed_today ?? 0, icon: TriangleAlert },
    { title: "Queued today", value: stats?.queued_today ?? 0, icon: Layers },
  ]);
  const maxHour = $derived(Math.max(1, ...(stats?.hourly ?? []).map((h) => h.count)));

  async function loadStats() {
    try {
      stats = await notifications.stats();
    } catch (e) {
      console.error("notification stats failed", e);
    }
  }
  async function loadList() {
    loading = true;
    try {
      const res = await notifications.queue({
        status: statusFilter || undefined,
        q: search.trim() || undefined,
        limit: PAGE_SIZE,
        offset: page * PAGE_SIZE,
      });
      items = res.items;
      total = res.total;
    } catch (e) {
      console.error("notification queue failed", e);
    } finally {
      loading = false;
    }
  }
  function refresh() {
    void loadStats();
    void loadList();
  }
  function onSearch(v: string) {
    search = v;
    page = 0;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void loadList(), 250);
  }
  function onStatus(v: string) {
    statusFilter = v;
    page = 0;
    void loadList();
  }
  function prevPage() {
    if (page > 0) {
      page--;
      void loadList();
    }
  }
  function nextPage() {
    if (page < pageCount - 1) {
      page++;
      void loadList();
    }
  }

  function statusClass(s: string): string {
    if (s === "sent") return "border-primary/30 bg-primary/10 text-primary";
    if (s === "failed") return "border-destructive/30 bg-destructive/10 text-destructive";
    if (s === "sending") return "border-primary/20 bg-primary/5 text-primary/80";
    return "border-border bg-muted/40 text-muted-foreground"; // queued
  }
  function hourLabel(h: string): string {
    return h.length >= 13 ? h.slice(11, 13) : h;
  }

  onMount(refresh);
</script>

<div class="flex flex-col gap-6 p-6">
  <PageHeader
    title="Notifications"
    description="Live view of the notification queue — no black box."
    icon={Gauge}
  >
    {#snippet actions()}
      <Button variant="outline" size="sm" onclick={refresh}>
        <RefreshCw class="size-4" /> Refresh
      </Button>
    {/snippet}
  </PageHeader>

  <!-- KPI tiles -->
  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
    {#each kpis as kpi (kpi.title)}
      {@const Icon = kpi.icon}
      <div class="rounded-xl border border-border bg-card p-5">
        <div
          class="flex size-10 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-inset ring-primary/20"
        >
          <Icon class="size-5" />
        </div>
        <div class="mt-4 text-3xl font-semibold tabular-nums tracking-tight">
          {#if stats}{kpi.value}{:else}<span class="text-muted-foreground/40">—</span>{/if}
        </div>
        <p class="mt-1 text-sm text-muted-foreground">{kpi.title}</p>
      </div>
    {/each}
  </div>

  <!-- Throughput today (enqueued per hour) -->
  <div class="rounded-xl border border-border bg-card p-5">
    <p class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
      Throughput today · enqueued per hour
    </p>
    {#if (stats?.hourly ?? []).length === 0}
      <p class="mt-4 text-sm text-muted-foreground">No activity yet today.</p>
    {:else}
      <div class="mt-4 flex h-28 items-end gap-1">
        {#each stats?.hourly ?? [] as h (h.hour)}
          <div class="flex flex-1 flex-col items-center gap-1" title="{hourLabel(h.hour)}:00 — {h.count}">
            <div
              class="w-full rounded-t bg-primary/70"
              style="height: {Math.max(2, (h.count / maxHour) * 96)}px"
            ></div>
            <span class="text-[9px] tabular-nums text-muted-foreground">{hourLabel(h.hour)}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Details table -->
  <div class="flex flex-wrap items-center gap-2">
    <div class="relative">
      <Search
        class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        value={search}
        oninput={(e) => onSearch(e.currentTarget.value)}
        placeholder="Search message / recipient…"
        class="h-9 w-64 pl-8"
      />
    </div>
    <select
      value={statusFilter}
      onchange={(e) => onStatus(e.currentTarget.value)}
      class="h-9 rounded-lg border border-border bg-card px-2.5 text-sm text-foreground outline-none"
    >
      <option value="">All statuses</option>
      <option value="queued">Queued</option>
      <option value="sending">Sending</option>
      <option value="sent">Sent</option>
      <option value="failed">Failed</option>
    </select>
  </div>

  <div class="rounded-xl border border-border">
    {#if loading && items.length === 0}
      <div class="p-10"><Loader label="Loading queue…" /></div>
    {:else if items.length === 0}
      <div class="p-6">
        <EmptyState title="Nothing here" description="No notifications match your filters." />
      </div>
    {:else}
      <Table.Root>
        <Table.Header class="sticky top-0 z-10 bg-card">
          <Table.Row>
            <Table.Head class="w-24">Status</Table.Head>
            <Table.Head>Message</Table.Head>
            <Table.Head class="w-32">Recipient</Table.Head>
            <Table.Head class="w-16 text-center">Tries</Table.Head>
            <Table.Head class="w-40">Created</Table.Head>
            <Table.Head class="w-40">Sent</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each items as row (row.id)}
            <Table.Row>
              <Table.Cell>
                <span
                  class="inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium capitalize {statusClass(row.status)}"
                >
                  {row.status}
                </span>
              </Table.Cell>
              <Table.Cell class="max-w-md">
                <span class="block truncate">{row.message}</span>
                {#if row.status === "failed" && row.result}
                  <span class="block truncate text-xs text-destructive">{row.result}</span>
                {/if}
                {#if row.image_key || row.video_key}
                  <span class="text-[10px] text-muted-foreground">📎 media</span>
                {/if}
              </Table.Cell>
              <Table.Cell class="text-muted-foreground">{row.target ?? "all"}</Table.Cell>
              <Table.Cell class="text-center tabular-nums text-muted-foreground">{row.attempts}</Table.Cell>
              <Table.Cell class="text-xs text-muted-foreground">{fmtTs(row.created_at)}</Table.Cell>
              <Table.Cell class="text-xs text-muted-foreground">
                {row.sent_at ? fmtTs(row.sent_at) : "—"}
              </Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    {/if}
  </div>

  {#if pageCount > 1}
    <div class="flex items-center justify-between text-sm text-muted-foreground">
      <span>Page {page + 1} of {pageCount} ({total} total)</span>
      <div class="flex gap-2">
        <Button variant="outline" size="sm" onclick={prevPage} disabled={page === 0}>
          <ChevronLeft class="size-4" /> Prev
        </Button>
        <Button variant="outline" size="sm" onclick={nextPage} disabled={page >= pageCount - 1}>
          Next <ChevronRight class="size-4" />
        </Button>
      </div>
    </div>
  {/if}
</div>
