<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { toast } from "svelte-sonner";

  import ScrollText from "@lucide/svelte/icons/scroll-text";
  import Search from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import { boxes as boxApi, type Box, type BoxLog } from "$lib/api";
  import { data } from "$lib/data";
  import { hasPerm } from "$lib/auth.svelte";

  const allowed = hasPerm("devices.read");

  let loading = $state(true);
  let boxesList = $state<Box[]>([]);
  let selectedBoxId = $state<string>("");
  let cams = $state<{ ip: string; name: string }[]>([]);

  // filters
  let deviceFilter = $state<string>(""); // camera ip, or "" = all
  let level = $state<string>("all"); // all | info | warn | error | debug
  let search = $state<string>("");
  let follow = $state(true);

  let lines = $state<BoxLog[]>([]);
  let logEl = $state<HTMLDivElement | null>(null);
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

  function logColor(lvl: string): string {
    const l = lvl.toLowerCase();
    if (l === "error") return "text-destructive";
    if (l === "warn") return "text-amber-500";
    if (l === "debug") return "text-muted-foreground/70";
    return "text-foreground/80";
  }

  async function selectBox(id: string) {
    if (!id) return;
    selectedBoxId = id;
    lines = [];
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

  // Auto-scroll to newest while following.
  $effect(() => {
    void shown.length;
    if (follow && logEl) logEl.scrollTop = logEl.scrollHeight;
  });
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
      <div class="flex items-center gap-1.5">
        <Label class="text-xs text-muted-foreground">Follow</Label>
        <Switch checked={follow} onCheckedChange={(v) => (follow = v)} />
      </div>
      <Button variant="outline" size="sm" class="h-8" onclick={clearView}>Clear</Button>
    </div>

    <!-- log stream (fills the page) -->
    <div
      bind:this={logEl}
      class="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border bg-muted/30 p-3 font-mono text-[11px] leading-relaxed"
    >
      {#if shown.length === 0}
        <p class="py-10 text-center text-muted-foreground">
          {lines.length === 0 ? "No logs yet — new lines stream in as the box reports them." : "No lines match the current filters."}
        </p>
      {:else}
        {#each shown as l, i (i)}
          <div class="flex gap-2 whitespace-pre-wrap break-words hover:bg-primary/5">
            <span class="shrink-0 text-muted-foreground/50">{l.ts.slice(11, 19)}</span>
            <span class="w-12 shrink-0 uppercase {logColor(l.level)}">{l.level}</span>
            {#if l.target}<span class="max-w-40 shrink-0 truncate text-muted-foreground/60" title={l.target}>{l.target}</span>{/if}
            <span class="{logColor(l.level)}">{l.message}</span>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>
