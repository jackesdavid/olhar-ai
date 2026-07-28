<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Rss from "@lucide/svelte/icons/rss";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Bell from "@lucide/svelte/icons/bell";
  import Users from "@lucide/svelte/icons/users";
  import Clapperboard from "@lucide/svelte/icons/clapperboard";
  import Workflow from "@lucide/svelte/icons/workflow";
  import LogIn from "@lucide/svelte/icons/log-in";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Send from "@lucide/svelte/icons/send";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Plus from "@lucide/svelte/icons/plus";

  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";

  import { events, type EventRow, type EventKind } from "$lib/api";
  import { dict, lang } from "$lib/i18n";
  import { fmtTs } from "$lib/utils";

  const t = $derived(dict[lang()].events);
  const c = $derived(dict[lang()].common);

  const PAGE = 50;

  let loading = $state(true);
  let loadingMore = $state(false);
  let items = $state<EventRow[]>([]);
  let kinds = $state<EventKind[]>([]);
  let hasMore = $state(false);

  let kindFilter = $state("");
  let severityFilter = $state("");
  let expanded = $state<Set<number>>(new Set());

  // Auto-refresh — silent reload of the newest page each tick.
  const REFRESH_OPTIONS = $derived([
    { value: "0", label: t.off },
    { value: "5", label: "5s" },
    { value: "10", label: "10s" },
    { value: "30", label: "30s" },
  ]);
  let autoSecs = $state(0);
  function autoLabel(secs: number): string {
    return REFRESH_OPTIONS.find((o) => o.value === String(secs))?.label ?? t.off;
  }

  const SEVERITIES = ["debug", "info", "warn", "error"];
  function severityLabel(s: string): string {
    if (s === "debug") return t.sevDebug;
    if (s === "warn") return t.sevWarn;
    if (s === "error") return t.sevError;
    return t.sevInfo;
  }

  function kindLabel(kind: string): string {
    return kinds.find((k) => k.kind === kind)?.label ?? kind;
  }
  // Each kind wears a face — reinforces that one stream carries every subsystem.
  function kindIcon(kind: string): typeof Rss {
    if (kind === "alert") return Bell;
    if (kind === "presence") return Users;
    if (kind.startsWith("clip")) return Clapperboard;
    if (kind.startsWith("workflow")) return Workflow;
    if (kind.startsWith("auth")) return LogIn;
    return Rss;
  }
  // Severity reads through the dot-field: a status dot, colour only for state (warn/amber, error/red).
  function dotClass(severity: string): string {
    if (severity === "error") return "bg-destructive";
    if (severity === "warn") return "bg-amber-500";
    if (severity === "debug") return "bg-muted-foreground/40";
    return "bg-primary"; // info
  }
  function line(e: EventRow): string {
    return e.title || e.message || kindLabel(e.kind);
  }
  function hasSubMessage(e: EventRow): boolean {
    return !!e.message && e.message !== line(e);
  }
  // A concise line of the entity refs an event carries.
  function meta(e: EventRow): string {
    const bits: string[] = [];
    if (e.camera) bits.push(e.camera);
    if (e.room_name) bits.push(e.room_name);
    bits.push(e.source);
    return bits.join(" · ");
  }
  function timeAgo(iso: string): string {
    const then = new Date(iso).getTime();
    if (!Number.isFinite(then)) return "";
    const s = Math.max(0, (Date.now() - then) / 1000);
    if (s < 60) return t.justNow;
    const m = Math.floor(s / 60);
    if (m < 60) return t.minutesAgo(m);
    const h = Math.floor(m / 60);
    if (h < 24) return t.hoursAgo(h);
    return t.daysAgo(Math.floor(h / 24));
  }

  function isAlert(e: EventRow): boolean {
    return e.kind === "alert";
  }
  // The typed payload as a plain record (or null), so the template stays logic-free.
  function payload(e: EventRow): Record<string, unknown> | null {
    return e.data && typeof e.data === "object" ? (e.data as Record<string, unknown>) : null;
  }
  type Delivery = { target?: string; ok?: boolean; at?: string };
  function deliveries(e: EventRow): Delivery[] {
    const d = payload(e)?.deliveries;
    return Array.isArray(d) ? (d as Delivery[]) : [];
  }
  function isDelivered(e: EventRow): boolean {
    return deliveries(e).some((d) => d.ok);
  }
  // Entity refs worth showing in the detail panel, skipping the empty ones.
  function refs(e: EventRow): { label: string; value: string }[] {
    const out: { label: string; value: string }[] = [];
    if (e.camera) out.push({ label: t.refCamera, value: e.camera });
    if (e.clip_id) out.push({ label: t.refClip, value: e.clip_id });
    if (e.room_name) out.push({ label: t.refRoom, value: e.room_name });
    if (e.graph_id != null) out.push({ label: t.refWorkflow, value: `#${e.graph_id}` });
    if (e.device_id) out.push({ label: t.refDevice, value: e.device_id });
    if (e.user_id) out.push({ label: t.refUser, value: e.user_id.slice(0, 8) });
    out.push({ label: t.refSource, value: e.source });
    out.push({ label: t.refOccurred, value: fmtTs(e.occurred_at) });
    return out;
  }
  // The payload minus `deliveries` (rendered on its own), pretty-printed — or "" when empty.
  function payloadJson(e: EventRow): string {
    const p = payload(e);
    if (!p) return "";
    const rest = { ...p };
    delete rest.deliveries;
    return Object.keys(rest).length ? JSON.stringify(rest, null, 2) : "";
  }

  function toggleExpand(id: number) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
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
      toast.error(e instanceof Error ? e.message : t.loadFailed);
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
    const iv = setInterval(silentReload, autoSecs * 1000);
    return () => clearInterval(iv);
  });

  function onKind(v: string) { kindFilter = v === "_all" ? "" : v; load(); }
  function onSeverity(v: string) { severityFilter = v === "_all" ? "" : v; load(); }

  // ---- actions (alert-kind events route through the /alerts endpoints) --------
  let deliveringId = $state<number | null>(null);
  async function doDeliver(e: EventRow) {
    deliveringId = e.id;
    try {
      toast.success(await events.deliver(e.id));
      await silentReload();
    } catch (err) {
      toast.error(t.deliveryFailed, { description: String(err) });
    } finally {
      deliveringId = null;
    }
  }

  let deleteTarget = $state<EventRow | null>(null);
  let deleting = $state(false);
  async function doDelete() {
    if (!deleteTarget) return;
    const target = deleteTarget;
    deleting = true;
    try {
      await events.remove(target.id);
      items = items.filter((x) => x.id !== target.id);
      deleteTarget = null;
      toast.success(t.eventDeleted);
    } catch (err) {
      toast.error(t.deleteFailed, { description: String(err) });
    } finally {
      deleting = false;
    }
  }

  // ---- raise an alert by hand -------------------------------------------------
  const LEVELS = $derived([
    { value: "info", label: t.levelInfo },
    { value: "warn", label: t.levelWarn },
    { value: "error", label: t.levelError },
    { value: "debug", label: t.levelDebug },
  ]);
  let createOpen = $state(false);
  let fLevel = $state("info");
  let fMessage = $state("");
  let fRoom = $state("");
  let fSource = $state("");
  let creating = $state(false);
  const fLevelLabel = $derived(LEVELS.find((l) => l.value === fLevel)?.label ?? t.levelInfo);

  function openCreate() {
    fLevel = "info"; fMessage = ""; fRoom = ""; fSource = "";
    createOpen = true;
  }
  async function doCreate() {
    if (!fMessage.trim()) { toast.error(t.enterMessage); return; }
    creating = true;
    try {
      await events.create(fLevel, fMessage.trim(), fRoom.trim() || null, fSource.trim() || null);
      toast.success(t.alertRaised);
      createOpen = false;
      await load();
    } catch (err) {
      toast.error(t.raiseFailed, { description: String(err) });
    } finally {
      creating = false;
    }
  }

  onMount(async () => {
    // Deep-link: /events?kind=alert&severity=warn pre-filters the feed.
    const q = new URLSearchParams(window.location.search);
    kindFilter = q.get("kind") ?? "";
    severityFilter = q.get("severity") ?? "";
    try { kinds = await events.kinds(); } catch { /* filter falls back to slugs */ }
    await load();
  });
</script>

<div class="flex flex-col gap-6 p-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={Rss}
  >
    {#snippet actions()}
      <Button variant="outline" size="sm" onclick={openCreate}>
        <Plus class="mr-2 h-4 w-4" /> {t.newAlert}
      </Button>
      <Select.Root type="single" value={String(autoSecs)} onValueChange={(v) => (autoSecs = Number(v))}>
        <Select.Trigger class="h-8 w-28 text-sm" title={t.autoRefresh}>{t.autoPrefix(autoLabel(autoSecs))}</Select.Trigger>
        <Select.Content>
          {#each REFRESH_OPTIONS as o (o.value)}
            <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
      <Button variant="outline" size="sm" onclick={load} disabled={loading}>
        {#if loading}<Busy class="mr-2" />{:else}<RefreshCw class="mr-2 h-4 w-4" />{/if}
        {t.refresh}
      </Button>
    {/snippet}
  </PageHeader>

  <!-- filters -->
  <div class="flex flex-wrap items-center gap-2">
    <Select.Root type="single" value={kindFilter || "_all"} onValueChange={onKind}>
      <Select.Trigger class="h-8 w-44 text-sm">{kindFilter ? kindLabel(kindFilter) : t.allKinds}</Select.Trigger>
      <Select.Content>
        <Select.Item value="_all" label={t.allKinds}>{t.allKinds}</Select.Item>
        {#each kinds as k (k.kind)}
          <Select.Item value={k.kind} label={k.label}>{k.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
    <Select.Root type="single" value={severityFilter || "_all"} onValueChange={onSeverity}>
      <Select.Trigger class="h-8 w-36 text-sm">{severityFilter ? severityLabel(severityFilter) : t.allSeverities}</Select.Trigger>
      <Select.Content>
        <Select.Item value="_all" label={t.allSeverities}>{t.allSeverities}</Select.Item>
        {#each SEVERITIES as s (s)}
          <Select.Item value={s} label={severityLabel(s)}>{severityLabel(s)}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </div>

  {#if loading}
    <Loader />
  {:else if items.length === 0}
    <EmptyState title={t.emptyTitle} description={t.emptyDescription} />
  {:else}
    <div class="overflow-hidden rounded-lg border border-border divide-y divide-white/5">
      {#each items as e (e.id)}
        {@const Icon = kindIcon(e.kind)}
        {@const open = expanded.has(e.id)}
        <div class="animate-in fade-in duration-200">
          <!-- summary row -->
          <button
            type="button"
            class="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-primary/5"
            onclick={() => toggleExpand(e.id)}
          >
            <span class="mt-1.5 h-2 w-2 shrink-0 rounded-full {dotClass(e.severity)}"></span>
            <span class="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary ring-1 ring-inset ring-primary/20">
              <Icon class="size-3.5" />
            </span>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <Badge variant="outline" class="border-primary/25 bg-primary/10 text-primary">{kindLabel(e.kind)}</Badge>
                <span class="truncate text-sm font-medium">{line(e)}</span>
                {#if isAlert(e) && isDelivered(e)}
                  <Badge variant="outline" class="border-primary/30 bg-primary/10 text-primary">{t.sent}</Badge>
                {/if}
              </div>
              {#if hasSubMessage(e)}
                <p class="mt-0.5 truncate text-sm text-muted-foreground">{e.message}</p>
              {/if}
              <p class="mt-0.5 text-xs text-muted-foreground">{meta(e)}</p>
            </div>
            <span class="shrink-0 whitespace-nowrap text-xs text-muted-foreground tabular-nums" title={fmtTs(e.occurred_at)}>
              {timeAgo(e.occurred_at)}
            </span>
            <ChevronRight class="mt-0.5 size-4 shrink-0 text-muted-foreground transition-transform {open ? 'rotate-90' : ''}" />
          </button>

          <!-- detail -->
          {#if open}
            <div class="border-t border-white/5 bg-muted/10 px-4 py-3 pl-16 animate-in fade-in slide-in-from-top-1 duration-150">
              <dl class="grid grid-cols-2 gap-x-6 gap-y-1.5 text-xs sm:grid-cols-3">
                {#each refs(e) as r (r.label)}
                  <div class="flex flex-col">
                    <dt class="font-semibold uppercase tracking-wide text-muted-foreground/70">{r.label}</dt>
                    <dd class="truncate tabular-nums" title={r.value}>{r.value}</dd>
                  </div>
                {/each}
              </dl>

              {#if isAlert(e) && deliveries(e).length > 0}
                <div class="mt-3">
                  <p class="text-xs font-semibold uppercase tracking-wide text-muted-foreground/70">{t.deliveries}</p>
                  <ul class="mt-1 space-y-0.5 text-xs">
                    {#each deliveries(e) as d (d.target)}
                      <li class="flex items-center gap-2">
                        <span class="h-1.5 w-1.5 rounded-full {d.ok ? 'bg-primary' : 'bg-destructive'}"></span>
                        <span class="text-muted-foreground">{d.target ?? t.beacon}</span>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/if}

              {#if payloadJson(e)}
                <pre class="mt-3 max-h-48 overflow-auto rounded-md border border-border bg-background p-2 text-[11px] leading-relaxed">{payloadJson(e)}</pre>
              {/if}

              <div class="mt-3 flex items-center gap-2">
                {#if isAlert(e)}
                  <Button variant="outline" size="sm" disabled={deliveringId === e.id} onclick={() => doDeliver(e)}>
                    {#if deliveringId === e.id}<Busy class="mr-2" />{:else}<Send class="mr-2 h-3.5 w-3.5" />{/if}
                    {t.deliverToRecipients}
                  </Button>
                {/if}
                <Button variant="ghost" size="sm" class="text-destructive hover:text-destructive" onclick={() => (deleteTarget = e)}>
                  <Trash2 class="mr-2 h-3.5 w-3.5" /> {t.delete}
                </Button>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>

    {#if hasMore}
      <div class="flex justify-center">
        <Button variant="outline" size="sm" onclick={loadMore} disabled={loadingMore}>
          {#if loadingMore}<Busy class="mr-2" />{/if}
          {t.loadMore}
        </Button>
      </div>
    {/if}
  {/if}
</div>

<!-- raise an alert -->
<Dialog.Root bind:open={createOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t.raiseTitle}</Dialog.Title>
      <Dialog.Description>{t.raiseDescription}</Dialog.Description>
    </Dialog.Header>
    <div class="space-y-4">
      <div class="space-y-2">
        <Label>{t.level}</Label>
        <Select.Root type="single" bind:value={fLevel}>
          <Select.Trigger class="w-full">{fLevelLabel}</Select.Trigger>
          <Select.Content>
            {#each LEVELS as l (l.value)}<Select.Item value={l.value} label={l.label}>{l.label}</Select.Item>{/each}
          </Select.Content>
        </Select.Root>
      </div>
      <div class="space-y-2">
        <Label for="ev-message">{t.message}</Label>
        <Input id="ev-message" bind:value={fMessage} placeholder={t.messagePlaceholder} />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="space-y-2">
          <Label for="ev-room">{t.roomOptional}</Label>
          <Input id="ev-room" bind:value={fRoom} placeholder={t.roomPlaceholder} />
        </div>
        <div class="space-y-2">
          <Label for="ev-source">{t.sourceOptional}</Label>
          <Input id="ev-source" bind:value={fSource} placeholder={t.sourcePlaceholder} />
        </div>
      </div>
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (createOpen = false)} disabled={creating}>{c.cancel}</Button>
      <Button onclick={doCreate} disabled={creating}>
        {#if creating}<Busy class="mr-2" />{t.raising}{:else}{t.raiseAlert}{/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- delete confirm -->
<Dialog.Root open={deleteTarget !== null} onOpenChange={(o) => { if (!o) deleteTarget = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t.deleteTitle}</Dialog.Title>
      <Dialog.Description>
        {t.deleteBefore}<span class="font-medium text-foreground">{deleteTarget ? line(deleteTarget) : ""}</span>{t.deleteAfter}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteTarget = null)} disabled={deleting}>{c.cancel}</Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>{deleting ? t.deleting : t.delete}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
