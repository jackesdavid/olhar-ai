<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Rss from "@lucide/svelte/icons/rss";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Select from "$lib/components/ui/select";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";

  import { events, type EventRow, type EventKind } from "$lib/api";
  import { fmtTs } from "$lib/utils";

  const PAGE = 50;

  let loading = $state(true);
  let loadingMore = $state(false);
  let items = $state<EventRow[]>([]);
  let kinds = $state<EventKind[]>([]);
  let hasMore = $state(false);

  let kindFilter = $state("");
  let severityFilter = $state("");

  // Auto-refresh — silent reload of the newest page each tick.
  const REFRESH_OPTIONS = [
    { value: "0", label: "Off" },
    { value: "5", label: "5s" },
    { value: "10", label: "10s" },
    { value: "30", label: "30s" },
  ];
  let autoSecs = $state(0);
  function autoLabel(secs: number): string {
    return REFRESH_OPTIONS.find((o) => o.value === String(secs))?.label ?? "Off";
  }

  const SEVERITIES = ["debug", "info", "warn", "error"];

  function kindLabel(kind: string): string {
    return kinds.find((k) => k.kind === kind)?.label ?? kind;
  }
  // Severity reads through the dot-field: a status dot, colour only for state (warn/amber, error/red).
  function dotClass(severity: string): string {
    if (severity === "error") return "bg-destructive";
    if (severity === "warn") return "bg-amber-500";
    if (severity === "debug") return "bg-muted-foreground/40";
    return "bg-primary"; // info
  }
  // A concise line of the entity refs an event carries.
  function meta(e: EventRow): string {
    const bits: string[] = [];
    if (e.camera) bits.push(e.camera);
    if (e.room_name) bits.push(e.room_name);
    bits.push(e.source);
    return bits.join(" · ");
  }
  function line(e: EventRow): string {
    return e.title || e.message || kindLabel(e.kind);
  }

  const opts = () => ({
    kind: kindFilter || undefined,
    severity: severityFilter || undefined,
    limit: PAGE,
  });

  async function load() {
    loading = true;
    try {
      const rows = await events.list(opts());
      items = rows;
      hasMore = rows.length === PAGE;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load events");
    } finally {
      loading = false;
    }
  }
  async function loadMore() {
    if (loadingMore || items.length === 0) return;
    loadingMore = true;
    try {
      const rows = await events.list({ ...opts(), after: items[items.length - 1].id });
      items = [...items, ...rows];
      hasMore = rows.length === PAGE;
    } catch (e) {
      toast.error(String(e));
    } finally {
      loadingMore = false;
    }
  }
  // Quiet reload of the newest page (auto-refresh); keeps last good data on a transient error.
  async function silentReload() {
    try {
      const rows = await events.list(opts());
      items = rows;
      hasMore = rows.length === PAGE;
    } catch { /* keep last good data */ }
  }

  $effect(() => {
    if (autoSecs <= 0) return;
    const t = setInterval(silentReload, autoSecs * 1000);
    return () => clearInterval(t);
  });

  function onKind(v: string) { kindFilter = v === "_all" ? "" : v; load(); }
  function onSeverity(v: string) { severityFilter = v === "_all" ? "" : v; load(); }

  onMount(async () => {
    try { kinds = await events.kinds(); } catch { /* filter falls back to slugs */ }
    await load();
  });
</script>

<div class="flex flex-col gap-6 p-6">
  <PageHeader
    title="Events"
    description="A unified feed of everything that happens across your site — alerts, presence, recordings, sign-ins."
    icon={Rss}
  >
    {#snippet actions()}
      <Select.Root type="single" value={String(autoSecs)} onValueChange={(v) => (autoSecs = Number(v))}>
        <Select.Trigger class="h-8 w-28 text-sm" title="Auto-refresh">Auto: {autoLabel(autoSecs)}</Select.Trigger>
        <Select.Content>
          {#each REFRESH_OPTIONS as o (o.value)}
            <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
      <Button variant="outline" size="sm" onclick={load} disabled={loading}>
        {#if loading}<Busy class="mr-2" />{:else}<RefreshCw class="mr-2 h-4 w-4" />{/if}
        Refresh
      </Button>
    {/snippet}
  </PageHeader>

  <!-- filters -->
  <div class="flex flex-wrap items-center gap-2">
    <Select.Root type="single" value={kindFilter || "_all"} onValueChange={onKind}>
      <Select.Trigger class="h-8 w-44 text-sm">{kindFilter ? kindLabel(kindFilter) : "All kinds"}</Select.Trigger>
      <Select.Content>
        <Select.Item value="_all" label="All kinds">All kinds</Select.Item>
        {#each kinds as k (k.kind)}
          <Select.Item value={k.kind} label={k.label}>{k.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
    <Select.Root type="single" value={severityFilter || "_all"} onValueChange={onSeverity}>
      <Select.Trigger class="h-8 w-36 text-sm">{severityFilter || "All severities"}</Select.Trigger>
      <Select.Content>
        <Select.Item value="_all" label="All severities">All severities</Select.Item>
        {#each SEVERITIES as s (s)}
          <Select.Item value={s} label={s}>{s}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>

  {#if loading}
    <Loader />
  {:else if items.length === 0}
    <EmptyState title="No events yet" description="Events appear here as things happen across your site." />
  {:else}
    <div class="rounded-lg border border-white/10 divide-y divide-white/5">
      {#each items as e (e.id)}
        <div class="flex items-start gap-3 px-4 py-3 animate-in fade-in duration-200">
          <span class="mt-1.5 h-2 w-2 shrink-0 rounded-full {dotClass(e.severity)}"></span>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <Badge variant="outline" class="border-primary/25 bg-primary/10 text-primary">{kindLabel(e.kind)}</Badge>
              <span class="truncate text-sm font-medium">{line(e)}</span>
            </div>
            {#if e.message && e.message !== line(e)}
              <p class="mt-0.5 truncate text-sm text-muted-foreground">{e.message}</p>
            {/if}
            <p class="mt-0.5 text-xs text-muted-foreground">{meta(e)}</p>
          </div>
          <span class="shrink-0 whitespace-nowrap text-xs text-muted-foreground tabular-nums">{fmtTs(e.occurred_at)}</span>
        </div>
      {/each}
    </div>

    {#if hasMore}
      <div class="flex justify-center">
        <Button variant="outline" size="sm" onclick={loadMore} disabled={loadingMore}>
          {#if loadingMore}<Busy class="mr-2" />{/if}
          Load more
        </Button>
      </div>
    {/if}
  {/if}
</div>
