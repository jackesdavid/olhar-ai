<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Film from "@lucide/svelte/icons/film";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";

  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";

  import { recordings, events as eventsApi, type Recording, type RecordingCamera, type EventRow } from "$lib/api";
  import {
    buildTracks,
    buildMarkers,
    markerSeverity,
    dayBounds,
    dayOrigin,
    hhmm,
    rulerTicks,
    widthOf,
    xOf,
    PX_PER_SEC_PRESETS,
    type TLClip,
    type TLMarker,
  } from "$lib/timeline";

  const DAY_SECS = 86_400;

  let loading = $state(true);
  let recs = $state<Recording[]>([]);
  let evs = $state<EventRow[]>([]);
  let cameras = $state<RecordingCamera[]>([]);
  let cameraFilter = $state(""); // "" = all
  let showAllKinds = $state(false); // false = footage-relevant only (hide auth.* / debug)
  let pxIdx = $state(1); // → 0.05 px/s
  let nowMs = $state(0);
  let hovered = $state<TLMarker | null>(null);

  function todayISO(): string {
    return isoOf(new Date());
  }
  function isoOf(d: Date): string {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  }
  let day = $state(todayISO());

  const pxPerSec = $derived(PX_PER_SEC_PRESETS[pxIdx]);
  const originMs = $derived(dayOrigin(day));
  const contentWidth = $derived(DAY_SECS * pxPerSec);
  const isToday = $derived(day === todayISO());
  const ticks = $derived(rulerTicks(originMs, originMs + DAY_SECS * 1000, pxPerSec));

  const filtered = $derived(cameraFilter ? recs.filter((r) => r.camera_ip === cameraFilter) : recs);
  const tracks = $derived(buildTracks(filtered, cameras));
  const nowX = $derived(xOf(nowMs, originMs, pxPerSec));

  // Events on the marker lane: drop noise (auth.* / debug) unless "All events" is on, and narrow
  // to the selected camera (site-wide events with no camera stay visible either way).
  function footageRelevant(e: EventRow): boolean {
    return showAllKinds || (!e.kind.startsWith("auth") && e.severity !== "debug");
  }
  function inCameraScope(e: EventRow): boolean {
    return !cameraFilter || !e.camera || e.camera === cameraFilter;
  }
  const shownEvents = $derived(evs.filter((e) => footageRelevant(e) && inCameraScope(e)));
  const markers = $derived(buildMarkers(shownEvents, pxPerSec));

  function cameraLabel(): string {
    if (!cameraFilter) return "All cameras";
    return cameras.find((c) => c.ip === cameraFilter)?.name ?? cameraFilter;
  }

  // ---- layout helpers (keep the markup logic-free) ---------------------------
  function clipLeft(c: TLClip): number {
    return xOf(c.startTs, originMs, pxPerSec);
  }
  function clipWidth(c: TLClip): number {
    return widthOf((c.endTs - c.startTs) / 1000, pxPerSec);
  }
  function clipClass(c: TLClip): string {
    // Uploaded clips read as durable ink; edge-only clips are dimmer with a dashed edge.
    return c.uploaded
      ? "border-primary/40 bg-primary/15 hover:bg-primary/25"
      : "border-dashed border-primary/30 bg-primary/5 hover:bg-primary/10";
  }
  function clipTitle(c: TLClip): string {
    return `${c.label} · ${hhmm(c.startTs)}–${hhmm(c.endTs)}${c.uploaded ? "" : " · edge only"}`;
  }
  function wideEnough(c: TLClip): boolean {
    return clipWidth(c) > 44;
  }

  // ---- marker lane helpers ---------------------------------------------------
  function markerLeft(m: TLMarker): number {
    return xOf(m.ts, originMs, pxPerSec);
  }
  // State colour only — never brand hue: info = ink, warn = amber, error = red (matches the feed).
  function sevClass(sev: "info" | "warn" | "error"): string {
    if (sev === "error") return "bg-destructive";
    if (sev === "warn") return "bg-amber-500";
    return "bg-primary";
  }
  function markerClass(m: TLMarker): string {
    return sevClass(m.severity);
  }
  function eventSevClass(e: EventRow): string {
    return sevClass(markerSeverity(e.severity));
  }
  function eventLine(e: EventRow): string {
    const who = e.camera || e.room_name || "site";
    const what = e.title || e.message || e.kind;
    return `${who} · ${what}`;
  }

  async function load() {
    loading = true;
    try {
      const { from, to } = dayBounds(day);
      const [page, evPage, cams] = await Promise.all([
        recordings.list({ from, to, sort: "started_at", dir: "asc", limit: 2000 }),
        eventsApi.list({ from, to, limit: 1000 }),
        cameras.length ? Promise.resolve(cameras) : recordings.cameras(),
      ]);
      recs = page.items;
      evs = evPage;
      cameras = cams;
      nowMs = Date.now();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load the timeline");
    } finally {
      loading = false;
    }
  }

  function shiftDay(delta: number) {
    const d = new Date(`${day}T00:00:00`);
    d.setDate(d.getDate() + delta);
    day = isoOf(d);
    load();
  }
  function goToday() {
    day = todayISO();
    load();
  }
  function zoom(delta: number) {
    pxIdx = Math.min(PX_PER_SEC_PRESETS.length - 1, Math.max(0, pxIdx + delta));
  }

  onMount(load);
</script>

<div class="flex h-full flex-col gap-4 p-6">
  <PageHeader
    title="Playback"
    description="Every camera on one timeline — scrub the day. Clips are ink blocks; the dashed lane is a gap with no footage."
    icon={Film}
  />

  <!-- toolbar -->
  <div class="flex shrink-0 flex-wrap items-center gap-2">
    <div class="flex items-center gap-1">
      <Button variant="outline" size="icon" class="size-8" title="Previous day" onclick={() => shiftDay(-1)}>
        <ChevronLeft class="size-4" />
      </Button>
      <Input type="date" value={day} onchange={(e) => { day = e.currentTarget.value; load(); }} class="h-8 w-40" />
      <Button variant="outline" size="icon" class="size-8" title="Next day" disabled={isToday} onclick={() => shiftDay(1)}>
        <ChevronRight class="size-4" />
      </Button>
      {#if !isToday}
        <Button variant="ghost" size="sm" class="h-8" onclick={goToday}>Today</Button>
      {/if}
    </div>

    <Select.Root type="single" value={cameraFilter || "_all"} onValueChange={(v) => (cameraFilter = v === "_all" ? "" : v)}>
      <Select.Trigger class="h-8 w-44 text-sm">{cameraLabel()}</Select.Trigger>
      <Select.Content>
        <Select.Item value="_all" label="All cameras">All cameras</Select.Item>
        {#each cameras as c (c.ip)}
          <Select.Item value={c.ip} label={c.name}>{c.name}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>

    <Button
      variant="outline"
      size="sm"
      class="h-8"
      title="Which events show on the marker lane"
      onclick={() => (showAllKinds = !showAllKinds)}
    >
      Events: {showAllKinds ? "all" : "footage"}
    </Button>

    <div class="ml-auto flex items-center gap-1">
      <Button variant="outline" size="icon" class="size-8" title="Zoom out" disabled={pxIdx === 0} onclick={() => zoom(-1)}>
        <ZoomOut class="size-4" />
      </Button>
      <Button
        variant="outline"
        size="icon"
        class="size-8"
        title="Zoom in"
        disabled={pxIdx === PX_PER_SEC_PRESETS.length - 1}
        onclick={() => zoom(1)}
      >
        <ZoomIn class="size-4" />
      </Button>
    </div>
  </div>

  {#if loading}
    <Loader />
  {:else if tracks.length === 0}
    <EmptyState
      title="No footage on this day"
      description="Nothing was recorded on {day}. Pick another day, or set a workflow to record on motion."
    />
  {:else}
    <div class="flex min-h-0 flex-1 overflow-hidden rounded-xl border border-border">
      <!-- fixed track-header column -->
      <div class="w-44 shrink-0 border-r border-border bg-card">
        <div class="h-8 border-b border-border"></div>
        <div class="flex h-7 items-center border-b border-border px-3">
          <span class="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">Events</span>
        </div>
        {#each tracks as t (t.cameraIp)}
          <div class="flex h-11 items-center gap-2 border-b border-border/60 px-3">
            <span class="size-2 shrink-0 rounded-full bg-primary/60"></span>
            <div class="min-w-0">
              <p class="truncate text-sm font-medium">{t.cameraName}</p>
              <p class="text-[11px] text-muted-foreground tabular-nums">{t.clips.length} clip{t.clips.length === 1 ? "" : "s"}</p>
            </div>
          </div>
        {/each}
      </div>

      <!-- scrolling time content -->
      <div class="min-w-0 flex-1 overflow-x-auto">
        <div class="relative" style="width: {contentWidth}px">
          <!-- ruler -->
          <div class="relative h-8 border-b border-border">
            {#each ticks as tk (tk.x)}
              <div class="absolute top-0 flex h-full flex-col items-start" style="left: {tk.x}px">
                <div class="h-2 w-px bg-border"></div>
                <span class="mt-0.5 pl-1 text-[10px] text-muted-foreground tabular-nums">{tk.label}</span>
              </div>
            {/each}
          </div>

          <!-- event marker lane: one dot per moment, coloured by the loudest severity -->
          <div class="relative h-7 border-b border-border">
            {#each markers as m (m.ts)}
              <div
                class="absolute top-1/2 z-10 -translate-x-1/2 -translate-y-1/2"
                style="left: {markerLeft(m)}px"
                role="button"
                tabindex="0"
                aria-label="{m.events.length} event(s) at {hhmm(m.ts)}"
                onmouseenter={() => (hovered = m)}
                onmouseleave={() => (hovered = null)}
                onfocus={() => (hovered = m)}
                onblur={() => (hovered = null)}
              >
                <span class="block size-2.5 rounded-full ring-2 ring-card {markerClass(m)}"></span>
                {#if m.events.length > 1}
                  <span class="absolute -right-2 -top-1 rounded-full bg-card px-1 text-[9px] font-semibold leading-tight text-muted-foreground tabular-nums ring-1 ring-border">
                    {m.events.length}
                  </span>
                {/if}
              </div>
            {/each}
          </div>

          <!-- tracks, with faint spine lines dropping from each marker -->
          <div class="relative">
            {#each markers as m (m.ts)}
              <div class="pointer-events-none absolute inset-y-0 w-px bg-primary/10" style="left: {markerLeft(m)}px"></div>
            {/each}
            {#each tracks as t (t.cameraIp)}
              <div class="relative h-11 border-b border-border/60">
                <!-- gap baseline: no footage shows through as a dashed lane -->
                <div class="absolute inset-x-0 top-1/2 border-t border-dashed border-border/50"></div>
                {#each t.clips as c (c.id)}
                  <div
                    class="absolute top-1.5 bottom-1.5 z-[1] flex items-center overflow-hidden rounded-md border px-1.5 transition-colors {clipClass(c)}"
                    style="left: {clipLeft(c)}px; width: {clipWidth(c)}px"
                    title={clipTitle(c)}
                  >
                    {#if wideEnough(c)}
                      <span class="truncate text-[11px] text-primary">{hhmm(c.startTs)}</span>
                    {/if}
                  </div>
                {/each}
              </div>
            {/each}
          </div>

          <!-- "now" guide (today only) -->
          {#if isToday && nowX >= 0 && nowX <= contentWidth}
            <div class="pointer-events-none absolute inset-y-0 w-px bg-primary/40" style="left: {nowX}px"></div>
          {/if}

          <!-- marker hover card: "same moment = same place" -->
          {#if hovered}
            <div
              class="pointer-events-none absolute z-30 w-60 -translate-x-1/2 rounded-lg border border-border bg-popover p-2 shadow-lg"
              style="left: {markerLeft(hovered)}px; top: 62px"
            >
              <p class="mb-1 text-[11px] font-semibold text-muted-foreground tabular-nums">
                {hhmm(hovered.ts)} · {hovered.events.length} event{hovered.events.length === 1 ? "" : "s"}
              </p>
              <ul class="space-y-0.5">
                {#each hovered.events.slice(0, 6) as e (e.id)}
                  <li class="flex items-center gap-1.5 text-xs">
                    <span class="size-1.5 shrink-0 rounded-full {eventSevClass(e)}"></span>
                    <span class="truncate">{eventLine(e)}</span>
                  </li>
                {/each}
                {#if hovered.events.length > 6}
                  <li class="text-[11px] text-muted-foreground">+{hovered.events.length - 6} more</li>
                {/if}
              </ul>
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>
