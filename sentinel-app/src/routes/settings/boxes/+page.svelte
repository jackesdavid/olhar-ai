<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import EmptyState from "$lib/components/empty-state.svelte";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import ArrowUpCircle from "@lucide/svelte/icons/arrow-up-circle";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Copy from "@lucide/svelte/icons/copy";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";

  import { boxes as boxApi, type Box } from "$lib/api";
  import { API_BASE } from "$lib/config";
  import { hasPerm } from "$lib/auth.svelte";
  import { pendingInstall } from "$lib/box-install.svelte";
  import { fmtTs } from "$lib/utils";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].boxes);
  const c = $derived(dict[lang()].common);

  const allowed = hasPerm("devices.write");

  let loading = $state(true);
  let list = $state<Box[]>([]);

  // Add form
  let name = $state("");
  let creating = $state(false);

  // Delete confirm
  let deleteTarget = $state<Box | null>(null);
  let deleting = $state(false);

  // Edge versioning — PER PLATFORM. macOS and Linux are published independently and are routinely
  // on different versions, so one shared "latest" is wrong for at least one box whenever they differ.
  let latestBy = $state<Record<string, string>>({});

  /** The published version for this box's platform, or "" when nothing is published for it. */
  function latestFor(b: Box): string {
    return (b.platform && latestBy[b.platform]) || "";
  }

  /** Compare dotted versions numerically. `!==` was the bug: a box AHEAD of the advertised version
   *  counted as outdated, so the UI offered a downgrade — which is what a box on 0.5.66 saw when
   *  the shared "latest" was Linux's 0.5.65. Only strictly-older is an update. */
  function isOlder(a: string, b: string): boolean {
    const pa = a.split(".").map(Number);
    const pb = b.split(".").map(Number);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
      const x = pa[i] ?? 0;
      const y = pb[i] ?? 0;
      if (Number.isNaN(x) || Number.isNaN(y)) return false; // unparseable → never offer an update
      if (x !== y) return x < y;
    }
    return false;
  }

  function outdated(b: Box): boolean {
    const target = latestFor(b);
    return !!b.version && !!target && isOlder(b.version, target);
  }

  async function load(silent = false) {
    if (!silent) loading = true;
    try {
      list = await boxApi.list();
      // One request per DISTINCT platform, not per box.
      const platforms = [...new Set(list.map((b) => b.platform).filter(Boolean))] as string[];
      const versions = await Promise.all(
        platforms.map((p) =>
          boxApi
            .latestVersion(p)
            .then((r) => r.version ?? "")
            .catch(() => ""),
        ),
      );
      latestBy = Object.fromEntries(platforms.map((p, i) => [p, versions[i]]));
    } catch (e) {
      if (!silent) toast.error(e instanceof Error ? e.message : t.loadFailed);
    } finally {
      if (!silent) loading = false;
    }
  }
  $effect(() => {
    if (allowed) load();
    else loading = false;
    // Stop any in-flight update poll when the view is torn down.
    return () => updatePollStop?.();
  });

  function buildCommand(token: string): string {
    // The edge is outbound-only now (#144) — no tailnet, so no auth key. It needs only where the
    // cloud is and its own identity.
    return [
      `curl -fsSL ${API_BASE}/install.sh | sudo sh -s -- \\`,
      `  --control ${API_BASE} \\`,
      `  --box-token ${token}`,
    ].join("\n");
  }

  async function create(e: SubmitEvent) {
    e.preventDefault();
    if (creating || !name.trim()) return;
    creating = true;
    try {
      const boxName = name.trim();
      const box = await boxApi.create(boxName);
      // Hand the one-time token + command to the dedicated install view (not a modal —
      // the command is long and gets copied to another machine).
      pendingInstall.value = {
        id: box.id,
        name: boxName,
        token: box.token,
        command: buildCommand(box.token),
        auto: true,
        needsKey: false,
      };
      name = "";
      await load();
      goto("/settings/boxes/install");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.createFailed);
    } finally {
      creating = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t.copied);
    } catch {
      toast.error(t.copyFailed);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget || deleting) return;
    deleting = true;
    try {
      await boxApi.del(deleteTarget.id);
      toast.success(t.removed(deleteTarget.name));
      deleteTarget = null;
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.removeFailed);
    } finally {
      deleting = false;
    }
  }

  function seen(b: Box): string {
    if (!b.last_seen) return t.neverConnected;
    return fmtTs(b.last_seen);
  }

  // ---- health (reported on the heartbeat) ----------------------------------
  // Friendly name for a reported GPU-decode method (#120).
  function hwLabel(m: string): string {
    return { vaapi: "VAAPI", qsv: "QuickSync", cuda: "CUDA", videotoolbox: "VideoToolbox", v4l2m2m: "V4L2" }[m] ?? m;
  }
  // The box has no working media backend at all — GStreamer absent (and, on a legacy box, ffmpeg too).
  // The edge links GStreamer now (#138); a box reporting it healthy needs nothing else.
  function mediaMissing(b: Box): boolean {
    return !b.gstreamer_ok && b.ffmpeg_ok === false;
  }
  // Boxes whose media path can't work. The edge reaches the cloud outbound-only now (#144 — SSE
  // commands + live over its own outbound connection), so there is no tailnet to be "offline" on.
  const unhealthy = $derived(list.filter((b) => mediaMissing(b)));
  function healthHint(b: Box): string {
    if (mediaMissing(b)) return t.healthHint(b.name);
    return "";
  }

  // ---- resource usage (CPU / memory / disk / GPU, reported on the heartbeat) -----
  type Gauge = { label: string; pct: number };
  function gauges(b: Box): Gauge[] {
    const g: Gauge[] = [];
    if (b.cpu_pct != null) g.push({ label: "CPU", pct: b.cpu_pct });
    if (b.mem_pct != null) g.push({ label: "MEM", pct: b.mem_pct });
    if (b.disk_pct != null) g.push({ label: "DSK", pct: b.disk_pct });
    if (b.gpu_pct != null) g.push({ label: "GPU", pct: b.gpu_pct });
    return g;
  }
  function hasMetrics(b: Box): boolean {
    return b.cpu_pct != null || b.mem_pct != null || b.disk_pct != null;
  }
  // Monochrome by default; amber when elevated, destructive when a box is under pressure.
  function barClass(pct: number): string {
    if (pct >= 85) return "bg-destructive";
    if (pct >= 70) return "bg-amber-500";
    return "bg-foreground/60";
  }

  // ---- restart / update ----------------------------------------------------
  let restartingId = $state<string | null>(null);
  async function restartBox(b: Box) {
    restartingId = b.id;
    try {
      await boxApi.restart(b.id);
      toast.success(t.restartRequested(b.name), { description: t.restartHint });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.restartFailed);
    } finally {
      restartingId = null;
    }
  }

  // The button stays loading from click until the box heartbeats the new version (~60–90s), so it
  // reflects the real end-to-end update, not just the ~200ms API call (#11). A silent 5s poll
  // watches `version`; a 3-minute cap avoids a button stuck forever if the edge never comes back.
  let updatingId = $state<string | null>(null);
  let updatePollStop: (() => void) | null = null;

  function stopUpdatePoll() {
    updatePollStop?.();
  }
  function startUpdatePoll(boxId: string, name: string, target: string) {
    stopUpdatePoll(); // never run two polls at once
    let attempts = 0;
    const id = setInterval(async () => {
      attempts += 1;
      await load(true);
      const box = list.find((b) => b.id === boxId);
      if (box && target && box.version === target) {
        stopUpdatePoll();
        toast.success(t.updated(name, target));
      } else if (attempts >= 36) {
        stopUpdatePoll();
        toast.warning(t.updateSlow(name));
      }
    }, 5000);
    updatePollStop = () => {
      clearInterval(id);
      updatePollStop = null;
      updatingId = null;
    };
  }

  async function updateBox(b: Box) {
    updatingId = b.id;
    // This box's platform, not a shared value — the poll below waits for the box to report exactly
    // this version, so a target from another platform would hang until the 3-minute cap.
    const target = latestFor(b);
    try {
      await boxApi.update(b.id);
      toast.info(t.updateStarted(b.name, target));
      startUpdatePoll(b.id, b.name, target);
    } catch (e) {
      updatingId = null;
      toast.error(e instanceof Error ? e.message : t.updateFailed);
    }
  }

  // Media backend is GStreamer everywhere now (card #138) — no per-box choice in the UI.
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={HardDrive}
  />

  {#if !allowed}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      {t.noPermission}
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    {#if unhealthy.length > 0}
      <div class="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-700 dark:text-amber-400">
        <TriangleAlert class="mt-0.5 size-4 shrink-0" />
        <div class="space-y-0.5">
          {#each unhealthy as b (b.id)}<p>{healthHint(b)}</p>{/each}
        </div>
      </div>
    {/if}
    <Card.Root>
      <Card.Header><Card.Title>{t.yourBoxes}</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">{t.colName}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colVersion}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colHealth}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colResources}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colLastSeen}</th>
              <th class="px-3 py-2 text-right font-medium"></th>
            </tr>
          </thead>
          <tbody>
            {#each list as b (b.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">
                  <div>{b.name}</div>
                  <button
                    class="interactive mt-0.5 inline-flex items-center gap-1 font-mono text-[10px] font-normal text-muted-foreground hover:text-foreground"
                    title={t.copyIdTitle(b.id)}
                    onclick={() => copy(b.id)}
                  >
                    <Copy class="size-3" />{b.id.slice(0, 8)}…
                  </button>
                </td>
                <td class="px-3 py-2">
                  {#if b.version}
                    <span class="font-mono text-xs tabular-nums text-muted-foreground">v{b.version}</span>
                    {#if outdated(b)}
                      <button
                        class="interactive ml-2 inline-flex items-center gap-1 rounded-full border border-amber-500/30 bg-amber-500/10 px-2 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400 disabled:opacity-50"
                        title={t.updateTo(latestFor(b))}
                        disabled={updatingId === b.id}
                        onclick={() => updateBox(b)}
                      >
                        {#if updatingId === b.id}
                          <Busy size={12} /> {t.updating}
                        {:else}
                          <ArrowUpCircle class="size-3" /> {t.updateArrow(latestFor(b))}
                        {/if}
                      </button>
                    {/if}
                  {:else}
                    <span class="text-xs text-muted-foreground">—</span>
                  {/if}
                </td>
                <td class="px-3 py-2">
                  <span class="flex flex-wrap items-center gap-1">
                    {#if b.platform}
                      <span class="inline-flex items-center rounded-full border border-border bg-muted px-2 py-0.5 text-[10px] font-medium text-muted-foreground" title={t.platformTitle}>
                        {b.platform}
                      </span>
                    {/if}
                    {#if b.hwaccel && b.hwaccel !== "none"}
                      <span class="inline-flex items-center rounded-full border border-foreground/30 bg-foreground/5 px-2 py-0.5 text-[10px] font-medium text-foreground" title={t.gpuDecodeTitle}>
                        {t.gpuDecode(hwLabel(b.hwaccel))}
                      </span>
                    {:else if b.hwaccel === "none"}
                      <span class="inline-flex items-center rounded-full border border-border bg-muted px-2 py-0.5 text-[10px] font-medium text-muted-foreground" title={t.cpuDecodeTitle}>
                        {t.cpuDecode}
                      </span>
                    {/if}
                    {#if mediaMissing(b)}
                      <span class="inline-flex items-center rounded-full border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-[10px] font-medium text-destructive" title={t.noMediaTitle}>
                        {t.noMedia}
                      </span>
                    {/if}
                  </span>
                </td>
                <td class="px-3 py-2">
                  {#if hasMetrics(b)}
                    <div class="flex min-w-[7.5rem] flex-col gap-1">
                      {#each gauges(b) as g (g.label)}
                        <div class="flex items-center gap-1.5" title="{g.label} {g.pct}%">
                          <span class="w-7 shrink-0 text-[9px] font-medium tabular-nums text-muted-foreground">{g.label}</span>
                          <span class="relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                            <span class="absolute inset-y-0 left-0 rounded-full {barClass(g.pct)}" style="width: {g.pct}%"></span>
                          </span>
                          <span class="w-8 shrink-0 text-right text-[9px] tabular-nums text-muted-foreground">{g.pct}%</span>
                        </div>
                      {/each}
                    </div>
                  {:else}
                    <span class="text-xs text-muted-foreground">—</span>
                  {/if}
                </td>
                <td class="px-3 py-2">
                  {#if b.last_seen}
                    <span class="text-muted-foreground">{seen(b)}</span>
                  {:else}
                    <Badge variant="secondary" class="text-[10px]">{t.neverConnected}</Badge>
                  {/if}
                </td>
                <td class="px-3 py-2 text-right">
                  <Button variant="ghost" size="icon" title={t.restartTitle} disabled={restartingId === b.id} onclick={() => restartBox(b)}>
                    {#if restartingId === b.id}<Busy />{:else}<RotateCcw class="size-4" />{/if}
                  </Button>
                  <Button variant="ghost" size="icon" title={t.removeTitle} onclick={() => (deleteTarget = b)}>
                    <Trash2 class="size-4" />
                  </Button>
                </td>
              </tr>
            {/each}
            {#if list.length === 0}
              <tr><td colspan={7}>
                <EmptyState
                  title={t.emptyTitle}
                  description={t.emptyDescription}
                />
              </td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>{t.addTitle}</Card.Title>
        <Card.Description>{t.addDescription}</Card.Description>
      </Card.Header>
      <Card.Content>
        <form class="space-y-4" onsubmit={create}>
          <div class="space-y-1.5">
            <Label for="box-name">{t.boxName}</Label>
            <Input id="box-name" bind:value={name} placeholder={t.boxNamePlaceholder} />
          </div>
          <Button type="submit" disabled={!name.trim() || creating}>
            {#if creating}<Busy />{:else}<Plus class="size-4" />{/if}
            {t.createBox}
          </Button>
        </form>
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<!-- Delete confirm -->
<Dialog.Root open={deleteTarget !== null} onOpenChange={(o) => { if (!o) deleteTarget = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t.deleteTitle}</Dialog.Title>
      <Dialog.Description>{t.deleteDescription(deleteTarget?.name ?? "")}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteTarget = null)}>{c.cancel}</Button>
      <Button variant="destructive" disabled={deleting} onclick={confirmDelete}>
        {#if deleting}<Busy />{/if}
        {t.remove}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
