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
    billing,
    type Recording,
    type RecordingListOpts,
    type RecordingCamera,
    type UploadState,
    type TenantStorage,
  } from "$lib/api";
  import { dict, lang, locale } from "$lib/i18n";

  const t = $derived(dict[lang()].recordings);
  const c = $derived(dict[lang()].common);

  // ---- query state -----------------------------------------------------------

  const PAGE_SIZE = 50;

  let loading = $state(true);
  let items = $state<Recording[]>([]);
  let total = $state(0);
  let cameraList = $state<RecordingCamera[]>([]);
  let storage = $state<TenantStorage | null>(null);

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

  // A clip in the cloud downloads FROM the cloud (presigned GET): no VPS egress, it works with the
  // edge offline — and, since the box may be set to drop its local copy once the clip is durable
  // (`delete_local_after_upload`), the edge proxy would simply 404 for a clip that is perfectly
  // safe. The proxy stays the path for a clip that never made it to the cloud.
  async function download(rec: Recording) {
    if (downloading !== null) return;
    downloading = rec.id;
    try {
      const url = await downloadUrl(rec);
      const a = document.createElement("a");
      a.href = url.href;
      a.download = `${rec.friendly_name || rec.clip_id}.mp4`;
      a.click();
      if (url.revoke) URL.revokeObjectURL(url.href);
    } catch (e) {
      toast.error(String(e));
    } finally {
      downloading = null;
    }
  }

  /** `revoke` marks an object URL we minted and must release; a presigned R2 URL is not ours. */
  async function downloadUrl(rec: Recording): Promise<{ href: string; revoke: boolean }> {
    if (rec.upload_state === "uploaded") {
      const { url } = await recordings.cloudUrl(rec.id);
      return { href: url, revoke: false };
    }
    const blob = await recordings.download(rec.id);
    return { href: URL.createObjectURL(blob), revoke: true };
  }

  // ---- clip state badge ------------------------------------------------------

  type BadgeSpec = { label: string; cls: string };
  const STATE_BADGE: Record<UploadState, BadgeSpec> = $derived({
    // Cloud = durable, inked primary. Local/in-transit = muted. Failed = destructive.
    uploaded:  { label: t.badgeCloud,     cls: "border-primary/25 bg-primary/10 text-primary" },
    uploading: { label: t.badgeInTransit, cls: "border-border bg-muted/40 text-muted-foreground animate-pulse" },
    pending:   { label: t.badgeLocal,     cls: "border-border bg-muted/40 text-muted-foreground" },
    failed:    { label: t.badgeFailed,    cls: "border-destructive/30 bg-destructive/10 text-destructive" },
  });
  const DELETING_BADGE: BadgeSpec = $derived({
    label: t.badgeDeleting,
    cls: "border-border bg-muted/40 text-muted-foreground animate-pulse",
  });
  function badgeFor(rec: Recording): BadgeSpec {
    if (rec.deleting) return DELETING_BADGE;
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
      toast.error(t.playFailed);
    } finally {
      playLoading = null;
    }
  }

  // ---- delete single ---------------------------------------------------------

  // Deletion is asynchronous: the request enqueues the cleanup jobs and the clip shows as
  // "deleting" until they finish, then disappears. So we reload (to pick up the deleting flag)
  // rather than drop the row optimistically, and reload once more shortly after to catch it gone.
  function reloadSoon() {
    setTimeout(() => load(), 4000);
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    const id = deleteTarget.id;
    deleting = id;
    deleteTarget = null;
    try {
      await recordings.del(id);
      toast.success(t.deletionStarted);
      await load();
      reloadSoon();
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
      selected = new Set();
      toast.success(t.bulkDeleting(res.deleted));
      await load();
      reloadSoon();
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

  // Dates follow the language the user picked, not the browser's own locale.
  function fmtDate(iso: string) {
    const d = new Date(iso);
    return (
      d.toLocaleDateString(locale(), { day: "2-digit", month: "short" }) +
      " " +
      d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })
    );
  }

  function fmtDay(dateStr: string) {
    const d = new Date(dateStr + "T00:00:00");
    return d.toLocaleDateString(locale(), {
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

  function groupByLabel(): string {
    if (groupBy === "camera") return t.groupCamera;
    if (groupBy === "day") return t.groupDay;
    return t.groupNone;
  }

  function cameraLabel(ip: string) {
    const cam = cameraList.find((c) => c.ip === ip);
    return cam ? cam.name : ip;
  }

  // ---- storage strip ---------------------------------------------------------
  // `stored_bytes` is the stock the customer sees "used"; `gb_month` (the billed average) stays
  // in the billing screen. This strip answers "am I near the cap?", not "what's the invoice".

  function fmtGB(bytes: number): string {
    const gb = bytes / 1e9;
    if (gb < 0.1) return `${(bytes / 1e6).toFixed(0)} MB`;
    return `${gb.toFixed(gb < 10 ? 2 : 1)} GB`;
  }
  function money(n: number, currency: string): string {
    return new Intl.NumberFormat(locale(), { style: "currency", currency }).format(n);
  }
  /** Percent of quota used, clamped to 100 for the bar width. 0 when there's no quota. */
  function usedPct(s: TenantStorage): number {
    if (s.included_storage_gb <= 0) return 0;
    return Math.min(100, (s.stored_bytes / 1e9 / s.included_storage_gb) * 100);
  }
  function barClass(s: TenantStorage): string {
    const pct = usedPct(s);
    if (pct >= 90) return "bg-destructive";
    if (pct >= 75) return "bg-amber-500";
    return "bg-primary";
  }
  /** True when the tenant's plan sells cloud recording at all. */
  function hasCloud(s: TenantStorage | null): boolean {
    return !!s && s.retention_days > 0;
  }

  onMount(async () => {
    try {
      cameraList = await recordings.cameras();
    } catch {
      // non-fatal — dropdown falls back gracefully
    }
    try {
      storage = await billing.storage();
    } catch {
      // non-fatal — the strip just doesn't render
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
      <Dialog.Title>{t.deleteTitle}</Dialog.Title>
      <Dialog.Description>
        {t.deleteDescription(deleteTarget?.friendly_name || deleteTarget?.clip_id || "")}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteTarget = null)}>{c.cancel}</Button>
      <Button variant="destructive" onclick={confirmDelete}>{t.delete}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- bulk delete confirm -->
<Dialog.Root open={bulkDeleteOpen} onOpenChange={(o: boolean) => { if (!o) bulkDeleteOpen = false; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t.bulkDeleteTitle(selected.size)}</Dialog.Title>
      <Dialog.Description>{t.bulkDeleteDescription}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (bulkDeleteOpen = false)}>{c.cancel}</Button>
      <Button variant="destructive" onclick={confirmBulkDelete}>
        {t.deleteCount(selected.size)}
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
      <video src={player.url} controls autoplay class="w-full rounded-lg bg-video-well">
        {t.cannotPlay}
      </video>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<div class="flex flex-col gap-6 p-6">
  <!-- header -->
  <PageHeader
    title={t.title}
    description={total > 0 ? t.countDescription(total) : t.emptyDescription}
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
          {t.deleteCount(selected.size)}
        </Button>
      {/if}
      <Button variant="outline" size="sm" onclick={load} disabled={loading}>
        {#if loading}<Busy class="mr-2" />{:else}<RefreshCw class="mr-2 h-4 w-4" />{/if}
        {t.refresh}
      </Button>
    {/snippet}
  </PageHeader>

  <!-- cloud storage usage strip -->
  {#if storage}
    {#if hasCloud(storage)}
      <div class="rounded-lg border border-border bg-card p-4">
        <div class="flex items-baseline justify-between gap-4">
          <div class="flex items-baseline gap-2">
            <span class="text-sm font-semibold tabular-nums">{fmtGB(storage.stored_bytes)}</span>
            <span class="text-sm text-muted-foreground">
              {t.storedOf(storage.included_storage_gb, storage.retention_days)}
            </span>
          </div>
          <span class="text-xs text-muted-foreground tabular-nums">
            {t.clipsInCloud(storage.clip_count)}
          </span>
        </div>
        <div class="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-muted">
          <div class="h-full rounded-full transition-all {barClass(storage)}" style="width: {usedPct(storage)}%"></div>
        </div>
        {#if storage.projected_overage > 0}
          <p class="mt-2 text-xs text-amber-500">
            {t.overQuota(money(storage.projected_overage, storage.currency))}
          </p>
        {:else}
          <p class="mt-2 text-xs text-muted-foreground">{t.uploadNote(storage.retention_days)}</p>
        {/if}
      </div>
    {:else}
      <div class="flex items-center justify-between rounded-lg border border-border bg-card p-4">
        <p class="text-sm text-muted-foreground">{t.noCloudPlan}</p>
        <Button href="/billing" variant="outline" size="sm">{t.seePlans}</Button>
      </div>
    {/if}
  {/if}

  <!-- filters + group-by bar -->
  <div class="flex flex-wrap items-end gap-3">
    <!-- camera dropdown -->
    <div class="flex flex-col gap-1">
      <Label class="text-xs text-muted-foreground">{t.camera}</Label>
      <Select.Root
        type="single"
        value={filterCamera || "_all"}
        onValueChange={(v) => { filterCamera = v === "_all" ? "" : v; }}
      >
        <Select.Trigger class="h-8 w-44 text-sm">
          {filterCamera ? cameraLabel(filterCamera) : t.allCameras}
        </Select.Trigger>
        <Select.Content>
          <Select.Item value="_all" label={t.allCameras}>{t.allCameras}</Select.Item>
          {#each cameraList as cam (cam.ip)}
            <Select.Item value={cam.ip} label={cam.name}>{cam.name}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>

    <div class="flex flex-col gap-1">
      <Label class="text-xs text-muted-foreground">{t.from}</Label>
      <Input type="date" class="h-8 w-36 text-sm" bind:value={filterFrom} />
    </div>
    <div class="flex flex-col gap-1">
      <Label class="text-xs text-muted-foreground">{t.to}</Label>
      <Input type="date" class="h-8 w-36 text-sm" bind:value={filterTo} />
    </div>
    <Button size="sm" class="h-8" onclick={applyFilters}>{t.apply}</Button>
    {#if filterCamera || filterFrom || filterTo}
      <Button size="sm" variant="ghost" class="h-8" onclick={resetFilters}>{t.clear}</Button>
    {/if}

    <div class="ml-auto flex items-center gap-2">
      <Label class="text-xs text-muted-foreground whitespace-nowrap">{t.groupBy}</Label>
      <Select.Root type="single" bind:value={groupBy}>
        <Select.Trigger class="h-8 w-32 text-sm">{groupByLabel()}</Select.Trigger>
        <Select.Content>
          <Select.Item value="none" label={t.groupNone}>{t.groupNone}</Select.Item>
          <Select.Item value="camera" label={t.groupCamera}>{t.groupCamera}</Select.Item>
          <Select.Item value="day" label={t.groupDay}>{t.groupDay}</Select.Item>
        </Select.Content>
      </Select.Root>
    </div>
  </div>

  {#if loading}
    <Loader />
  {:else if items.length === 0}
    <EmptyState title={t.emptyTitle} description={t.emptyHint} />
  {:else}
    {@const groups = groupedItems()}
    <div class="rounded-lg border border-border overflow-hidden">
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <!-- select-all checkbox -->
            <Table.Head class="w-10">
              <button
                class="flex items-center justify-center"
                onclick={toggleAll}
                title={allSelected ? t.deselectAll : t.selectAll}
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
                {t.colName}
                {#if sortIcon("friendly_name") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("friendly_name") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("camera_ip")}>
                {t.camera}
                {#if sortIcon("camera_ip") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("camera_ip") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("started_at")}>
                {t.colDate}
                {#if sortIcon("started_at") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("started_at") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("duration_secs")}>
                {t.colDuration}
                {#if sortIcon("duration_secs") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("duration_secs") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head>
              <button class="flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort("size_bytes")}>
                {t.colSize}
                {#if sortIcon("size_bytes") === "asc"}<ChevronUp class="h-3 w-3" />
                {:else if sortIcon("size_bytes") === "desc"}<ChevronDown class="h-3 w-3" />
                {:else}<ChevronsUpDown class="h-3 w-3 opacity-40" />
                {/if}
              </button>
            </Table.Head>
            <Table.Head class="w-28">{t.colStorage}</Table.Head>
            <Table.Head class="w-28 text-right">{t.colActions}</Table.Head>
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
                        title={t.play}
                        disabled={playLoading === rec.id || rec.deleting}
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
                      title={t.download}
                      disabled={downloading === rec.id || rec.deleting}
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
                      title={t.delete}
                      disabled={deleting === rec.id || rec.deleting}
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
        <span>{t.pageOf(page + 1, pageCount, total)}</span>
        <div class="flex gap-2">
          <Button variant="outline" size="sm" onclick={prevPage} disabled={page === 0}>
            <ChevronLeft class="h-4 w-4" />
            {t.prev}
          </Button>
          <Button variant="outline" size="sm" onclick={nextPage} disabled={page >= pageCount - 1}>
            {t.next}
            <ChevronRight class="h-4 w-4" />
          </Button>
        </div>
      </div>
    {/if}
  {/if}
</div>
