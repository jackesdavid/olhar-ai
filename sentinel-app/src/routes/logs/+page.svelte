<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { toast } from "svelte-sonner";

  import ScrollText from "@lucide/svelte/icons/scroll-text";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import * as Table from "$lib/components/ui/table";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import { boxes as boxApi, type Box, type BoxLog } from "$lib/api";
  import { data } from "$lib/data";
  import { hasPerm } from "$lib/auth.svelte";

  const allowed = hasPerm("devices.read");
  const PER_PAGE = 50;

  let loading = $state(true);
  let boxesList = $state<Box[]>([]);
  let selectedBoxId = $state<string>("");
  let cams = $state<{ ip: string; name: string }[]>([]);

  // filters
  let deviceFilter = $state<string>(""); // camera ip, or "" = all
  let level = $state<string>("all"); // all | info | warn | error | debug
  let search = $state<string>("");

  // table state
  let sortDesc = $state(true); // newest-first by default
  let page = $state(1);

  let lines = $state<BoxLog[]>([]);
  let es: EventSource | null = null;
  const CAP = 6000;

  const selectedBox = $derived(boxesList.find((b) => b.id === selectedBoxId) ?? null);

  // A camera's log lines mention its IP dotted (edge: "192.168.0.132") or underscored
  // (go2rtc stream: "cam_192_168_0_132") — match either when filtering by device.
  const shown = $derived(
    lines.filter((l) => {
      if (level !== "all" && l.level.toLowerCase() !== level) return false;
      if (deviceFilter) {
        const hay = `${l.message} ${l.target}`.toLowerCase();
        if (!hay.includes(deviceFilter) && !hay.includes(deviceFilter.replaceAll(".", "_"))) return false;
      }
      const q = search.trim().toLowerCase();
      if (q && !l.message.toLowerCase().includes(q)) return false;
      return true;
    }),
  );

  // Sort by creation time (ts is RFC3339 text → lexicographic order matches chronological).
  const sorted = $derived(
    [...shown].sort((a, b) => (sortDesc ? b.ts.localeCompare(a.ts) : a.ts.localeCompare(b.ts))),
  );
  const totalPages = $derived(Math.max(1, Math.ceil(sorted.length / PER_PAGE)));
  const pageRows = $derived(sorted.slice((page - 1) * PER_PAGE, page * PER_PAGE));

  // Reset to the first page whenever the filter set changes.
  $effect(() => {
    void deviceFilter;
    void level;
    void search;
    page = 1;
  });
  // Keep the page in range as rows stream in or filters narrow.
  $effect(() => {
    if (page > totalPages) page = totalPages;
  });

  function logColor(lvl: string): string {
    const l = lvl.toLowerCase();
    if (l === "error") return "text-destructive";
    if (l === "warn") return "text-amber-500";
    if (l === "debug") return "text-muted-foreground/70";
    return "text-foreground/80";
  }
  // RFC3339 → "YYYY-MM-DD HH:MM:SS" for the table cell.
  function fmtTs(ts: string): string {
    return ts.slice(0, 19).replace("T", " ");
  }

  async function selectBox(id: string) {
    if (!id) return;
    selectedBoxId = id;
    lines = [];
    page = 1;
    es?.close();
    es = null;
    try {
      lines = await boxApi.logs(id, 1500);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load logs");
    }
    es = new EventSource(boxApi.logsStreamUrl(id));
    es.onmessage = (ev) => {
      try {
        const l = JSON.parse(ev.data) as BoxLog;
        lines = [...lines, l].slice(-CAP);
      } catch {
        /* ignore malformed frames */
      }
    };
  }

  function clearView() {
    lines = [];
    page = 1;
  }

  onMount(() => {
    if (!allowed) {
      loading = false;
      return;
    }
    const qs = new URLSearchParams(window.location.search);
    const wantBox = qs.get("box");
    deviceFilter = qs.get("device") ?? "";
    (async () => {
      try {
        const [bxs, devs] = await Promise.all([boxApi.list(), data.listDevices().catch(() => [])]);
        boxesList = bxs;
        cams = devs.map((d) => ({ ip: d.ip, name: d.name }));
        const initial = (wantBox && bxs.some((b) => b.id === wantBox) ? wantBox : bxs[0]?.id) ?? "";
        if (initial) await selectBox(initial);
      } catch (e) {
        toast.error(e instanceof Error ? e.message : "Failed to load boxes");
      } finally {
        loading = false;
      }
    })();
  });
  onDestroy(() => es?.close());
</script>

<div class="flex h-full flex-col gap-3 p-4">
  <!-- header -->
  <div class="flex flex-wrap items-center gap-3">
    <div class="flex items-center gap-2">
      <ScrollText class="size-5 text-primary" />
      <h1 class="text-xl font-semibold tracking-tight">Edge logs</h1>
    </div>
    {#if boxesList.length > 1}
      <select
        class="h-8 rounded-md border border-border bg-background px-2 text-sm"
        value={selectedBoxId}
        onchange={(e) => selectBox(e.currentTarget.value)}
      >
        {#each boxesList as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
      </select>
    {:else if selectedBox}
      <span class="text-sm text-muted-foreground">{selectedBox.name}</span>
    {/if}
    <p class="text-sm text-muted-foreground">Live tail of the edge appliance (sentinel-edge + go2rtc).</p>
  </div>

  {#if !allowed}
    <EmptyState title="No access" description="You don't have permission to view edge logs." />
  {:else if loading}
    <Loader label="Loading logs…" />
  {:else if boxesList.length === 0}
    <EmptyState title="No edge boxes" description="Install an edge box from Settings → Edge boxes to see its logs." />
  {:else}
    <!-- filter bar -->
    <div class="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-card/40 p-2">
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Device</Label>
        <select
          class="h-8 max-w-48 rounded-md border border-border bg-background px-2 text-sm"
          bind:value={deviceFilter}
        >
          <option value="">All devices</option>
          {#each cams as c (c.ip)}<option value={c.ip}>{c.name} ({c.ip})</option>{/each}
        </select>
      </div>
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Level</Label>
        <select class="h-8 rounded-md border border-border bg-background px-2 text-sm" bind:value={level}>
          <option value="all">all</option>
          <option value="info">info</option>
          <option value="warn">warn</option>
          <option value="error">error</option>
          <option value="debug">debug</option>
        </select>
      </div>
      <div class="relative min-w-48 flex-1">
        <Search class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input bind:value={search} placeholder="Search message text…" class="h-8 pl-8" />
        {#if search}
          <button
            class="interactive absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            title="Clear search"
            onclick={() => (search = "")}
          >
            <X class="size-3.5" />
          </button>
        {/if}
      </div>
      <span class="text-xs tabular-nums text-muted-foreground">{shown.length} / {lines.length}</span>
      <Button variant="outline" size="sm" class="h-8" onclick={clearView}>Clear</Button>
    </div>

    <!-- data table (full width, fills the page) -->
    <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border">
      <div class="min-h-0 flex-1 overflow-auto">
        <Table.Root class="w-full">
          <Table.Header class="sticky top-0 z-10 bg-card">
            <Table.Row>
              <Table.Head class="w-44">
                <button class="interactive inline-flex items-center gap-1" onclick={() => (sortDesc = !sortDesc)}>
                  Time
                  {#if sortDesc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}
                </button>
              </Table.Head>
              <Table.Head class="w-16">Level</Table.Head>
              <Table.Head class="w-40">Source</Table.Head>
              <Table.Head>Message</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#if pageRows.length === 0}
              <Table.Row>
                <Table.Cell colspan={4} class="py-10 text-center text-muted-foreground">
                  {lines.length === 0
                    ? "No logs yet — new lines stream in as the box reports them."
                    : "No lines match the current filters."}
                </Table.Cell>
              </Table.Row>
            {:else}
              {#each pageRows as l (l.ts + l.message)}
                <Table.Row class="align-top">
                  <Table.Cell class="whitespace-nowrap font-mono text-[11px] tabular-nums text-muted-foreground">
                    {fmtTs(l.ts)}
                  </Table.Cell>
                  <Table.Cell class="font-mono text-[11px] uppercase {logColor(l.level)}">{l.level}</Table.Cell>
                  <Table.Cell class="max-w-40 truncate font-mono text-[11px] text-muted-foreground/70" title={l.target}>
                    {l.target}
                  </Table.Cell>
                  <Table.Cell class="whitespace-pre-wrap break-words font-mono text-[11px] {logColor(l.level)}">
                    {l.message}
                  </Table.Cell>
                </Table.Row>
              {/each}
            {/if}
          </Table.Body>
        </Table.Root>
      </div>

      <!-- pagination -->
      <div class="flex items-center justify-between border-t border-border bg-card/40 px-3 py-1.5 text-xs text-muted-foreground">
        <span class="tabular-nums">{sorted.length} line(s) · page {page} / {totalPages}</span>
        <div class="flex items-center gap-1">
          <Button variant="outline" size="sm" class="h-7 px-2" disabled={page <= 1} onclick={() => (page = 1)}>First</Button>
          <Button variant="outline" size="icon" class="size-7" disabled={page <= 1} onclick={() => (page -= 1)}>
            <ChevronLeft class="size-4" />
          </Button>
          <Button variant="outline" size="icon" class="size-7" disabled={page >= totalPages} onclick={() => (page += 1)}>
            <ChevronRight class="size-4" />
          </Button>
        </div>
      </div>
    </div>
  {/if}
</div>
