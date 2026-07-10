<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Clapperboard from "@lucide/svelte/icons/clapperboard";
  import Download from "@lucide/svelte/icons/download";
  import Play from "@lucide/svelte/icons/play";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Busy from "$lib/components/busy.svelte";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Square from "@lucide/svelte/icons/square";
  import CheckSquare from "@lucide/svelte/icons/check-square";

  import { Button } from "$lib/components/ui/button";
  import * as Table from "$lib/components/ui/table";
  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import {
    recordings,
    type Recording,
    type RecordingListOpts,
    type RecordingCamera,
    type UploadState,
  } from "$lib/api";

  // ---- query state -----------------------------------------------------------

  const PAGE_SIZE = 50;

  let loading = $state(true);
  let items = $state<Recording[]>([]);
  let total = $state(0);
  let cameraList = $state<RecordingCamera[]>([]);

  let filterCamera = $state("");
  let filterFrom   = $state("");
  let filterTo     = $state("");
  let sortField    = $state("started_at");
  let sortDir      = $state<"asc" | "desc">("desc");
  let page         = $state(0);
  let groupBy      = $state<"none" | "camera" | "day">("none");

  // ---- selection state -------------------------------------------------------

  let selected     = $state<Set<number>>(new Set());
  let bulkDeleting = $state(false);

  const allSelected = $derived(
    items.length > 0 && items.every((r) => selected.has(r.id))
  );
  const someSelected = $derived(selected.size > 0);

  function toggleAll() {
    if (allSelected) {
      selected = new Set();
    } else {
      selected = new Set(items.map((r) => r.id));
    }
  }

  function toggleRow(id: number) {
    const s = new Set(selected);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selected = s;
  }

  // ---- delete state ----------------------------------------------------------

  let deleteTarget = $state<Recording | null>(null);
  let deleting     = $state<number | null>(null);
  let downloading  = $state<number | null>(null);

  // ---- load ------------------------------------------------------------------

  async function load() {
    loading = true;
    try {
      const opts: RecordingListOpts = {
        sort:   sortField,
        dir:    sortDir,
        limit:  PAGE_SIZE,
        offset: page * PAGE_SIZE,
      };
      if (filterCamera) opts.camera_ip = filterCamera;
      if (filterFrom)   opts.from      = filterFrom + "T00:00:00Z";
      if (filterTo)     opts.to        = filterTo   + "T23:59:59Z";
      const res = await recordings.list(opts);
      items = res.items;
      total = res.total;
      selected = new Set();
    } catch (e) {
      toast.error(String(e));
    } finally {
      loading = false;
    }
  }

  function applyFilters() {
    page = 0;
    load();
  }

  function resetFilters() {
    filterCamera = "";
    filterFrom   = "";
    filterTo     = "";
    page         = 0;
    load();
  }

  // ---- sort ------------------------------------------------------------------

  function toggleSort(field: string) {
    if (sortField === field) {
      sortDir = sortDir === "desc" ? "asc" : "desc";
    } else {
      sortField = field;
      sortDir = "desc";
    }
    page = 0;
    load();
  }

  // ---- pagination ------------------------------------------------------------

  const pageCount = $derived(Math.ceil(total / PAGE_SIZE));

  function prevPage() { if (page > 0) { page--; load(); } }
  function nextPage() { if (page < pageCount - 1) { page++; load(); } }

  // ---- download --------------------------------------------------------------

  async function download(rec: Recording) {
    if (downloading !== null) return;
    downloading = rec.id;
    try {
      const blob = await recordings.download(rec.id);
      const url  = URL.createObjectURL(blob);
      const a    = document.createElement("a");
      a.href     = url;
      a.download = `${rec.friendly_name || rec.clip_id}.mp4`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      toast.error(String(e));
    } finally {
      downloading = null;
    }
  }

  // ---- clip state badge ------------------------------------------------------

  type BadgeSpec = { label: string; cls: string };
  const STATE_BADGE: Record<UploadState, BadgeSpec> = {
    // Cloud = durable, inked primary. Local/in-transit = muted. Failed = destructive.
    uploaded:  { label: "Cloud",      cls: "border-primary/25 bg-primary/10 text-primary" },
    uploading: { label: "In transit", cls: "border-white/15 bg-muted/40 text-muted-foreground animate-pulse" },
    pending:   { label: "Local",      cls: "border-white/15 bg-muted/40 text-muted-foreground" },
    failed:    { label: "Failed",     cls: "border-destructive/30 bg-destructive/10 text-destructive" },
  };
  function badgeFor(rec: Recording): BadgeSpec {
    return STATE_BADGE[rec.upload_state] ?? STATE_BADGE.pending;
  }

  // ---- playback --------------------------------------------------------------
  // Cloud clips stream straight from R2 (presigned GET) — no VPS egress, and they play even
  // when the edge is offline, which is the whole reason for the durable copy. Local clips have
  // no browser-streamable URL yet (the edge proxy needs the JWT bearer), so their action stays
  // Download until Phase 2b wires an authenticated proxy stream.

  let player      = $state<{ rec: Recording; url: string } | null>(null);
  let playLoading = $state<number | null>(null);

  function canPlay(rec: Recording): boolean {
    return rec.upload_state === "uploaded";
  }

  async function play(rec: Recording) {
    if (playLoading !== null) return;
    playLoading = rec.id;
    try {
      const { url } = await recordings.cloudUrl(rec.id);
      player = { rec, url };
    } catch (e) {
      toast.error("Could not open this clip for playback.");
    } finally {
      playLoading = null;
    }
  }

  // ---- delete single ---------------------------------------------------------

  async function confirmDelete() {
    if (!deleteTarget) return;
    const id = deleteTarget.id;
    deleting = id;
    deleteTarget = null;
    try {
      await recordings.del(id);
      items = items.filter((r) => r.id !== id);
      total = Math.max(0, total - 1);
      selected.delete(id);
      selected = new Set(selected);
      toast.success("Recording deleted");
    } catch (e) {
      toast.error(String(e));
    } finally {
      deleting = null;
    }
  }

  // ---- bulk delete -----------------------------------------------------------

  let bulkDeleteOpen = $state(false);

  async function confirmBulkDelete() {
    bulkDeleteOpen = false;
    const ids = [...selected];
    if (ids.length === 0) return;
    bulkDeleting = true;
    try {
      const res = await recordings.bulkDel(ids);
      items = items.filter((r) => !ids.includes(r.id));
      total = Math.max(0, total - res.deleted);
      selected = new Set();
      toast.success(`${res.deleted} recording${res.deleted !== 1 ? "s" : ""} deleted`);
    } catch (e) {
      toast.error(String(e));
    } finally {
      bulkDeleting = false;
    }
  }

  // ---- grouping --------------------------------------------------------------

  type Group = { key: string; label: string; rows: Recording[] };

  function groupedItems(): Group[] {
    if (groupBy === "none") return [{ key: "_all", label: "", rows: items }];
    const map = new Map<string, Recording[]>();
    for (const r of items) {
      const key =
        groupBy === "camera"
          ? r.camera_name || r.camera_ip
          : groupBy === "day"
          ? r.started_at.slice(0, 10)
          : r.camera_ip;
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(r);
    }
    return [...map.entries()].map(([key, rows]) => ({
      key,
      label: groupBy === "day" ? fmtDay(key) : key,
      rows,
    }));
  }

  // ---- format helpers --------------------------------------------------------

  function fmtDuration(secs: number) {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return m > 0 ? `${m}m ${s}s` : `${s}s`;
  }

  function fmtSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function fmtDate(iso: string) {
    const d = new Date(iso);
    return (
      d.toLocaleDateString(undefined, { day: "2-digit", month: "short" }) +
      " " +
      d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    );
  }

  function fmtDay(dateStr: string) {
    const d = new Date(dateStr + "T00:00:00");
    return d.toLocaleDateString(undefined, {
      weekday: "long",
      day: "numeric",
      month: "long",
      year: "numeric",
    });
  }

  function sortIcon(field: string) {
    if (sortField !== field) return "none";
    return sortDir === "desc" ? "desc" : "asc";
  }

  function cameraLabel(ip: string) {
    const cam = cameraList.find((c) => c.ip === ip);
    return cam ? cam.name : ip;
  }

  onMount(async () => {
    try {
      cameraList = await recordings.cameras();
    } catch {
      // non-fatal — dropdown falls back gracefully
    }
    await load();
  });
</script>

<!-- delete single confirm -->
<Dialog.Root
  open={!!deleteTarget}
  onOpenChange={(o: boolean) => { if (!o) deleteTarget = null; }}
>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete recording?</Dialog.Title>
      <Dialog.Description>
        "{deleteTarget?.friendly_name || deleteTarget?.clip_id}" will be permanently removed from
        the edge disk. This cannot be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteTarget = null)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmDelete}>Delete</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- bulk delete confirm -->
<Dialog.Root open={bulkDeleteOpen} onOpenChange={(o: boolean) => { if (!o) bulkDeleteOpen = false; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete {selected.size} recording{selected.size !== 1 ? "s" : ""}?</Dialog.Title>
      <Dialog.Description>
        These clips will be permanently removed from the edge disk. This cannot be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (bulkDeleteOpen = false)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmBulkDelete}>
        Delete {selected.size}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- cloud clip player -->
<Dialog.Root open={player !== null} onOpenChange={(o: boolean) => { if (!o) player = null; }}>
  <Dialog.Content class="max-w-3xl">
    <Dialog.Header>
      <Dialog.Title>{player?.rec.friendly_name || player?.rec.clip_id}</Dialog.Title>
      <Dialog.Description>
        {player ? `${cameraLabel(player.rec.camera_ip)} · ${fmtDate(player.rec.started_at)}` : ""}
      </Dialog.Description>
    </Dialog.Header>
    {#if player}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={player.url} controls autoplay class="w-full rounded-lg bg-black">
        Your browser cannot play this clip.
      </video>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<div class="flex flex-col gap-6 p-6">
  <!-- header -->
  <PageHeader
    title="Recordings"
    description={total > 0
      ? `${total} recording${total === 1 ? "" : "s"} saved from your cameras.`
      : "Clips saved from your cameras."}
    icon={Clapperboard}
  >
    {#snippet actions()}
      {#if someSelected}
        <Button
          variant="destructive"
          size="sm"
          disabled={bulkDeleting}
          onclick={() => (bulkDeleteOpen = true)}
        >
          {#if bulkDeleting}
            <Busy class="mr-2" />
          {:else}
            <Trash2 class="mr-2 h-4 w-4" />
          {/if}
          Delete {selected.size}
        </Button>
      {/if}
      <Button variant="outline" size="sm" onclick={load} disabled={loading}>
        {#if loading}<Busy class="mr-2" />{:else}<RefreshCw class="mr-2 h-4 w-4" />{/if}
        Refresh
      </Button>
    {/snippet}
  </PageHeader>

  <!-- filters + group-by bar -->
  <div class="flex flex-wrap items-end gap-3">
    <!-- camera dropdown -->
    <div class="flex flex-col gap-1">
      <Label class="text-xs text-muted-foreground">Camera</Label>
      <Select.Root
        type="single"
        value={filterCamera || "_all"}
        onValueChange={(v) => { filterCamera = v === "_all" ? "" : v; }}
      >
        <Select.Trigger class="h-8 w-44 text-sm">
          {filterCamera ? cameraLabel(filterCamera) : "All cameras"}
        </Select.Trigger>
        <Select.Content>
          <Select.Item value="_all" label="All cameras">All cameras</Select.Item>
          {#each cameraList as cam (cam.ip)}
            <Select.Item value={cam.ip} label={cam.name}>{cam.name}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    <div class="flex flex-col gap-1">
      <Label class="text-xs text-muted-foreground">From</Label>
      <Input type="date" class="h-8 w-36 text-sm" bind:value={filterFrom} />
    </div>
    <div class="flex flex-col gap-1">
      <Label class="text-xs text-muted-foreground">To</Label>
      <Input type="date" class="h-8 w-36 text-sm" bind:value={filterTo} />
    </div>
    <Button size="sm" class="h-8" onclick={applyFilters}>Apply</Button>
    {#if filterCamera || filterFrom || filterTo}
      <Button size="sm" variant="ghost" class="h-8" onclick={resetFilters}>Clear</Button>
    {/if}

    <div class="ml-auto flex items-center gap-2">
      <Label class="text-xs text-muted-foreground whitespace-nowrap">Group by</Label>
      <Select.Root type="single" bind:value={groupBy}>
        <Select.Trigger class="h-8 w-32 text-sm">
          {groupBy === "none" ? "None" : groupBy === "camera" ? "Camera" : "Day"}
        </Select.Trigger>
        <Select.Content>
          <Select.Item value="none" label="None">None</Select.Item>
          <Select.Item value="camera" label="Camera">Camera</Select.Item>
          <Select.Item value="day" label="Day">Day</Select.Item>
        </Select.Content>
      </Select.Root>
    </div>
  </div>

  {#if loading}
    <Loader />
  {:else if items.length === 0}
    <EmptyState
      title="No recordings found"
      description="Recordings appear here once a workflow triggers the Start Recording node and a clip is finalised."
    />
  {:else}
    {@const groups = groupedItems()}
    <div class="rounded-lg border border-white/10 overflow-hidden">
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <!-- select-all checkbox -->
            <Table.Head class="w-10">
              <button
                class="flex items-center justify-center"
                onclick={toggleAll}
                title={allSelected ? "Deselect all" : "Select all"}
              >
                {#if allSelected}
                  <CheckSquare class="h-4 w-4 text-primary" />
                {:else}
                  <Square class="h-4 w-4 text-muted-foreground/60" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head class="w-48">
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("friendly_name")}>
                Name
                {#if sortIcon("friendly_name") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("friendly_name") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("camera_ip")}>
                Camera
                {#if sortIcon("camera_ip") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("camera_ip") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("started_at")}>
                Date
                {#if sortIcon("started_at") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("started_at") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("duration_secs")}>
                Duration
                {#if sortIcon("duration_secs") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("duration_secs") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("size_bytes")}>
                Size
                {#if sortIcon("size_bytes") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("size_bytes") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head class="w-28">Storage</Table.Head>
            <Table.Head class="w-28 text-right">Actions</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each groups as group (group.key)}
            {#if groupBy !== "none"}
              <Table.Row class="bg-muted/20 hover:bg-muted/20">
                <Table.Cell colspan={8} class="py-1.5 text-xs font-semibold text-muted-foreground uppercase tracking-wide">
                  {group.label}
                </Table.Cell>
              </Table.Row>
            {/if}
            {#each group.rows as rec (rec.id)}
              {@const isDeleting = deleting === rec.id || bulkDeleting && selected.has(rec.id)}
              <Table.Row class={isDeleting ? "opacity-40 pointer-events-none" : ""}>
                <!-- row checkbox -->
                <Table.Cell class="py-2">
                  <button
                    class="flex items-center justify-center"
                    onclick={() => toggleRow(rec.id)}
                  >
                    {#if selected.has(rec.id)}
                      <CheckSquare class="h-4 w-4 text-primary" />
                    {:else}
                      <Square class="h-4 w-4 text-muted-foreground/40" />
                    {/if}
                  </button>
                </Table.Cell>
                <Table.Cell class="font-medium">
                  {rec.friendly_name || "—"}
                </Table.Cell>
                <Table.Cell class="text-sm">
                  <span class="font-medium">{rec.camera_name || cameraLabel(rec.camera_ip)}</span>
                  {#if rec.camera_name}
                    <span class="block text-xs text-muted-foreground font-mono">{rec.camera_ip}</span>
                  {/if}
                </Table.Cell>
                <Table.Cell class="text-sm text-muted-foreground whitespace-nowrap">
                  {fmtDate(rec.started_at)}
                </Table.Cell>
                <Table.Cell class="text-sm">{fmtDuration(rec.duration_secs)}</Table.Cell>
                <Table.Cell class="text-sm text-muted-foreground">{fmtSize(rec.size_bytes)}</Table.Cell>
                <Table.Cell>
                  {@const badge = badgeFor(rec)}
                  <Badge variant="outline" class={badge.cls}>{badge.label}</Badge>
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex justify-end gap-1">
                    {#if canPlay(rec)}
                      <Button
                        variant="ghost"
                        size="icon"
                        title="Play"
                        disabled={playLoading === rec.id}
                        onclick={() => play(rec)}
                      >
                        {#if playLoading === rec.id}
                          <Busy />
                        {:else}
                          <Play class="h-4 w-4" />
                        {/if}
                      </Button>
                    {/if}
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Download"
                      disabled={downloading === rec.id}
                      onclick={() => download(rec)}
                    >
                      {#if downloading === rec.id}
                        <Busy />
                      {:else}
                        <Download class="h-4 w-4" />
                      {/if}
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Delete"
                      disabled={deleting === rec.id}
                      onclick={() => (deleteTarget = rec)}
                    >
                      {#if deleting === rec.id}
                        <Busy />
                      {:else}
                        <Trash2 class="h-4 w-4 text-destructive" />
                      {/if}
                    </Button>
                  </div>
                </Table.Cell>
              </Table.Row>
            {/each}
          {/each}
        </Table.Body>
      </Table.Root>
    </div>

    <!-- pagination -->
    {#if pageCount > 1}
      <div class="flex items-center justify-between text-sm text-muted-foreground">
        <span>Page {page + 1} of {pageCount} ({total} recordings)</span>
        <div class="flex gap-2">
          <Button variant="outline" size="sm" onclick={prevPage} disabled={page === 0}>
            <ChevronLeft class="h-4 w-4" />
            Prev
          </Button>
          <Button variant="outline" size="sm" onclick={nextPage} disabled={page >= pageCount - 1}>
            Next
            <ChevronRight class="h-4 w-4" />
          </Button>
        </div>
      </div>
    {/if}
  {/if}
</div>
