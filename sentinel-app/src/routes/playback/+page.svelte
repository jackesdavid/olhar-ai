<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount, tick } from "svelte";
  import { toast } from "svelte-sonner";

  import Film from "@lucide/svelte/icons/film";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";

  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";

  import { recordings, events as eventsApi, type Recording, type RecordingCamera, type EventRow } from "$lib/api";
  import {
    buildTracks,
    buildMarkers,
    mergeSegments,
    markerSeverity,
    dayBounds,
    dayOrigin,
    hhmm,
    rulerTicks,
    widthOf,
    xOf,
    tsOf,
    PX_PER_SEC_PRESETS,
    type TLClip,
    type TLTrack,
    type TLSegment,
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

  // ---- track segment layout (merged recording sessions; keep markup logic-free) ----
  function segLeft(s: TLSegment): number {
    return xOf(s.startTs, originMs, pxPerSec);
  }
  function segWidth(s: TLSegment, next?: TLSegment): number {
    const w = widthOf((s.endTs - s.startTs) / 1000, pxPerSec);
    if (!next) return w;
    // Never overrun the next session's start — leaves a 1px sliver of real gap between sessions.
    const cap = ((next.startTs - s.startTs) / 1000) * pxPerSec - 1;
    return Math.max(1, Math.min(w, cap));
  }
  function segClass(s: TLSegment): string {
    // Uploaded sessions read as durable ink; a run with edge-only footage is dimmer + dashed.
    return s.uploaded
      ? "border-primary/40 bg-primary/15 hover:bg-primary/25"
      : "border-dashed border-primary/30 bg-primary/5 hover:bg-primary/10";
  }
  function segTitle(s: TLSegment): string {
    const clips = `${s.clipCount} clip${s.clipCount === 1 ? "" : "s"}`;
    return `${hhmm(s.startTs)}–${hhmm(s.endTs)} · ${clips}${s.uploaded ? "" : " · has edge-only footage"}`;
  }
  function segWide(s: TLSegment, next?: TLSegment): boolean {
    return segWidth(s, next) > 44;
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
      // Open on real footage: earliest clip under the playhead, first camera active.
      const starts = page.items.map((r) => Date.parse(r.started_at)).filter(Number.isFinite);
      playheadMs = starts.length ? Math.min(...starts) : originMs;
      activeCamera = tracks[0]?.cameraIp ?? "";
      loadedClip = null;
      playing = false;
      void resolveVideo();
      void revealPlayhead();
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
  async function zoom(delta: number) {
    // Anchor the playhead: keep it at the same on-screen x across the zoom so you don't lose your place.
    const anchor = scrollEl ? playheadX - scrollEl.scrollLeft : 0;
    pxIdx = Math.min(PX_PER_SEC_PRESETS.length - 1, Math.max(0, pxIdx + delta));
    await tick();
    if (scrollEl) scrollEl.scrollLeft = Math.max(0, playheadX - anchor);
  }

  // ── playback (Phase 3): active camera + playhead + transport ────────────────
  let activeCamera = $state("");
  let playheadMs = $state(0);
  let playing = $state(false);
  let videoEl = $state<HTMLVideoElement | null>(null);
  let videoSrc = $state("");
  let videoBusy = $state(false);
  let videoErr = $state("");
  let loadedClip = $state<TLClip | null>(null);
  let contentEl = $state<HTMLDivElement | null>(null);
  let scrollEl = $state<HTMLDivElement | null>(null);
  let seekOnLoad = 0;
  let objectUrl = "";
  let raf = 0;
  let dragging = false;
  // True from a seek until the video has actually reached the new spot — while set, the rAF loop
  // must NOT write the playhead back from the (still-old) video time, so the click feels instant.
  let seeking = false;

  const activeTrack = $derived(tracks.find((t) => t.cameraIp === activeCamera) ?? null);
  const activeClip = $derived(clipAt(activeTrack, playheadMs));
  const playheadX = $derived(xOf(playheadMs, originMs, pxPerSec));

  function recById(id: number): Recording | undefined {
    return recs.find((r) => r.id === id);
  }
  /** The clip covering `ms` on a track, or null (a gap). */
  function clipAt(track: TLTrack | null, ms: number): TLClip | null {
    if (!track) return null;
    return track.clips.find((c) => ms >= c.startTs && ms < c.endTs) ?? null;
  }
  /** Start of the first clip that begins after `ms` (skips the current clip + any gap). */
  function nextClipStart(track: TLTrack | null, ms: number): number | null {
    return track?.clips.find((c) => c.startTs > ms)?.startTs ?? null;
  }
  /** Start of the clip to jump back to: this clip's own start if we're inside it, else the one before. */
  function prevClipStart(track: TLTrack | null, ms: number): number | null {
    if (!track) return null;
    const here = clipAt(track, ms);
    if (here && ms > here.startTs + 750) return here.startTs;
    let prev: number | null = null;
    for (const c of track.clips) {
      if (c.startTs < ms - 750) prev = c.startTs;
      else break;
    }
    return prev;
  }

  function releaseObjectUrl() {
    if (objectUrl) {
      URL.revokeObjectURL(objectUrl);
      objectUrl = "";
    }
  }

  /** Point the preview at the clip under the playhead (or clear it in a gap). Same clip → seek in
   *  place; new clip → resolve its source (presigned R2 when uploaded, edge blob otherwise). */
  async function resolveVideo() {
    const clip = clipAt(activeTrack, playheadMs);
    if (!clip) {
      loadedClip = null;
      videoSrc = "";
      videoErr = "";
      seeking = false;
      releaseObjectUrl();
      return;
    }
    if (loadedClip && clip.id === loadedClip.id && videoEl) {
      videoEl.currentTime = Math.max(0, (playheadMs - clip.startTs) / 1000);
      seeking = false;
      return;
    }
    videoBusy = true;
    videoErr = "";
    seekOnLoad = Math.max(0, (playheadMs - clip.startTs) / 1000);
    try {
      releaseObjectUrl();
      const rec = recById(clip.id);
      let src = "";
      if (rec && rec.upload_state === "uploaded") {
        src = (await recordings.cloudUrl(clip.id)).url;
      } else {
        objectUrl = URL.createObjectURL(await recordings.download(clip.id));
        src = objectUrl;
      }
      loadedClip = clip;
      videoSrc = src;
    } catch {
      videoErr = "Couldn't load this clip.";
      loadedClip = null;
      videoSrc = "";
    } finally {
      videoBusy = false;
    }
  }

  function onLoadedMeta() {
    if (!videoEl) return;
    videoEl.currentTime = Math.max(0, seekOnLoad);
    seeking = false;
    if (playing) void videoEl.play();
  }

  function clampMs(ms: number): number {
    return Math.min(originMs + DAY_SECS * 1000, Math.max(originMs, ms));
  }
  function seekTo(ms: number, keepPlaying = playing) {
    playheadMs = clampMs(ms); // instant visual move — the video catches up after
    playing = keepPlaying;
    seeking = true;
    void resolveVideo();
  }
  function setActive(ip: string) {
    activeCamera = ip;
    loadedClip = null;
    void resolveVideo();
  }
  function togglePlay() {
    if (!activeClip) {
      const ns = nextClipStart(activeTrack, playheadMs);
      if (ns == null) return;
      seekTo(ns, true);
      return;
    }
    playing = !playing;
    if (playing) void videoEl?.play();
    else videoEl?.pause();
  }
  // Scroll the timeline so the playhead is comfortably in view (after a clip jump or first load).
  async function revealPlayhead() {
    await tick();
    if (!scrollEl) return;
    const x = playheadX;
    const left = scrollEl.scrollLeft;
    const w = scrollEl.clientWidth;
    if (x < left + 48 || x > left + w - 48) {
      scrollEl.scrollLeft = Math.max(0, x - w / 2);
    }
  }
  function jumpNextClip() {
    const ns = nextClipStart(activeTrack, playheadMs);
    if (ns == null) {
      toast.info("No later clip on this camera.");
      return;
    }
    seekTo(ns);
    void revealPlayhead();
  }
  function jumpPrevClip() {
    const ps = prevClipStart(activeTrack, playheadMs);
    if (ps == null) {
      toast.info("No earlier clip on this camera.");
      return;
    }
    seekTo(ps);
    void revealPlayhead();
  }
  /** A finished clip flows into the next one (skipping any gap), else playback stops. */
  function onEnded() {
    const from = loadedClip ? loadedClip.endTs - 1 : playheadMs;
    const ns = nextClipStart(activeTrack, from);
    if (ns != null) seekTo(ns, true);
    else {
      playing = false;
      if (loadedClip) playheadMs = loadedClip.endTs;
    }
  }

  // Smoothly advance the playhead from the playing video (rAF, not the ~4Hz timeupdate).
  $effect(() => {
    if (!playing) {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      return;
    }
    const step = () => {
      if (videoEl && loadedClip && !videoEl.paused && !seeking) {
        playheadMs = loadedClip.startTs + videoEl.currentTime * 1000;
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    };
  });

  // Keep a valid active camera as the track set changes.
  $effect(() => {
    if (tracks.length && !tracks.some((t) => t.cameraIp === activeCamera)) {
      activeCamera = tracks[0].cameraIp;
    }
  });

  // ---- seek by clicking / keyboard on the timeline ---------------------------
  function timelineToMs(clientX: number): number {
    if (!contentEl) return playheadMs;
    return tsOf(clientX - contentEl.getBoundingClientRect().left, originMs, pxPerSec);
  }
  function onTimelineClick(e: MouseEvent) {
    seekTo(timelineToMs(e.clientX));
  }
  function onTimelineKey(e: KeyboardEvent) {
    const stepS = e.shiftKey ? 60 : 5;
    if (e.key === "ArrowRight") { e.preventDefault(); seekTo(playheadMs + stepS * 1000); }
    else if (e.key === "ArrowLeft") { e.preventDefault(); seekTo(playheadMs - stepS * 1000); }
    else if (e.key === ".") { e.preventDefault(); jumpNextClip(); }
    else if (e.key === ",") { e.preventDefault(); jumpPrevClip(); }
    else if (e.key === " " || e.key === "Enter") { e.preventDefault(); togglePlay(); }
  }
  function startDrag(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    e.stopPropagation();
  }
  function onDrag(e: PointerEvent) {
    if (dragging) playheadMs = clampMs(timelineToMs(e.clientX));
  }
  function endDrag() {
    if (!dragging) return;
    dragging = false;
    seekTo(playheadMs);
  }

  /** Wall-clock `HH:MM:SS` of the playhead (seconds into the local day). */
  function hms(ms: number): string {
    const s = Math.max(0, Math.floor((ms - originMs) / 1000));
    return `${String(Math.floor(s / 3600) % 24).padStart(2, "0")}:${String(Math.floor(s / 60) % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
  }

  onMount(() => {
    load();
    return () => {
      releaseObjectUrl();
      if (raf) cancelAnimationFrame(raf);
    };
  });
</script>

<svelte:window onpointermove={onDrag} onpointerup={endDrag} />

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
  </div>

  {#if loading}
    <Loader />
  {:else if tracks.length === 0}
    <EmptyState
      title="No footage on this day"
      description="Nothing was recorded on {day}. Pick another day, or set a workflow to record on motion."
    />
  {:else}
    <div class="flex min-h-0 flex-1 flex-col gap-3">
      <!-- stage: active-camera preview + camera tiles -->
      <div class="flex flex-col gap-3 lg:flex-row">
        <div class="relative aspect-video w-full shrink-0 overflow-hidden rounded-xl border border-border bg-black lg:max-w-xl">
          {#if videoSrc}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              bind:this={videoEl}
              src={videoSrc}
              class="h-full w-full object-contain"
              playsinline
              onloadedmetadata={onLoadedMeta}
              onseeked={() => (seeking = false)}
              onended={onEnded}
              onplay={() => (playing = true)}
              onpause={() => (playing = false)}
            ></video>
          {:else}
            <div class="flex h-full w-full items-center justify-center px-6 text-center text-sm text-muted-foreground">
              {#if videoErr}
                {videoErr}
              {:else}
                No footage on {activeTrack?.cameraName ?? "this camera"} at {hms(playheadMs)} — jump to the next clip.
              {/if}
            </div>
          {/if}
          {#if videoBusy}
            <div class="absolute inset-0 grid place-items-center bg-black/40"><Busy /></div>
          {/if}
          <div class="pointer-events-none absolute left-0 top-0 flex items-center gap-2 p-2">
            <span class="rounded-md bg-black/55 px-2 py-1 text-xs font-medium text-white">{activeTrack?.cameraName ?? "—"}</span>
            <span class="rounded-md bg-black/55 px-2 py-1 text-xs text-white tabular-nums">{hms(playheadMs)}</span>
          </div>
        </div>

        <div class="grid flex-1 auto-rows-min grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-2 xl:grid-cols-3">
          {#each tracks as t (t.cameraIp)}
            <button
              type="button"
              class="flex items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors {t.cameraIp === activeCamera
                ? 'border-primary bg-primary/5'
                : 'border-border hover:bg-muted/40'}"
              onclick={() => setActive(t.cameraIp)}
            >
              <span class="size-2 shrink-0 rounded-full {clipAt(t, playheadMs) ? 'bg-primary' : 'bg-muted-foreground/40'}"></span>
              <span class="min-w-0">
                <span class="block truncate text-sm font-medium">{t.cameraName}</span>
                <span class="block text-[11px] text-muted-foreground">{clipAt(t, playheadMs) ? "recording" : "no footage"}</span>
              </span>
            </button>
          {/each}
        </div>
      </div>

      <!-- transport -->
      <div class="flex shrink-0 items-center gap-2 rounded-xl border border-border bg-card px-3 py-2">
        <Button variant="outline" size="icon" class="size-9" title="Previous clip (,)" onclick={jumpPrevClip}>
          <SkipBack class="size-4" />
        </Button>
        <Button size="icon" class="size-9" title={playing ? "Pause (Space)" : "Play (Space)"} onclick={togglePlay}>
          {#if playing}<Pause class="size-4" />{:else}<Play class="size-4" />{/if}
        </Button>
        <Button variant="outline" size="icon" class="size-9" title="Next clip (.)" onclick={jumpNextClip}>
          <SkipForward class="size-4" />
        </Button>
        <span class="ml-2 text-sm font-semibold tabular-nums">{hms(playheadMs)}</span>
        <span class="text-xs text-muted-foreground">· {activeTrack?.cameraName ?? "—"}</span>
        <div class="ml-auto flex items-center gap-2">
          <span class="hidden text-[11px] text-muted-foreground lg:block">Space · , / . clip · ← → step</span>
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

      <!-- timeline -->
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
      <div class="min-w-0 flex-1 overflow-x-auto" bind:this={scrollEl}>
        <div
          class="relative"
          style="width: {contentWidth}px"
          bind:this={contentEl}
          role="slider"
          tabindex="0"
          aria-label="Playhead — click or use arrow keys to scrub"
          aria-valuemin={0}
          aria-valuemax={DAY_SECS}
          aria-valuenow={Math.round((playheadMs - originMs) / 1000)}
          onclick={onTimelineClick}
          onkeydown={onTimelineKey}
        >
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
              {@const segs = mergeSegments(t.clips)}
              <div class="relative h-11 border-b border-border/60">
                <!-- gap baseline: no footage shows through as a dashed lane -->
                <div class="absolute inset-x-0 top-1/2 border-t border-dashed border-border/50"></div>
                {#each segs as s, i (s.startTs)}
                  <div
                    class="absolute top-1.5 bottom-1.5 z-[1] flex items-center overflow-hidden rounded-md border px-1.5 transition-colors {segClass(s)}"
                    style="left: {segLeft(s)}px; width: {segWidth(s, segs[i + 1])}px"
                    title={segTitle(s)}
                  >
                    {#if segWide(s, segs[i + 1])}
                      <span class="truncate text-[11px] text-primary tabular-nums">{hhmm(s.startTs)}</span>
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

          <!-- playhead: ink line + draggable head -->
          {#if playheadX >= 0 && playheadX <= contentWidth}
            <div class="pointer-events-none absolute inset-y-0 z-20 w-px bg-primary" style="left: {playheadX}px">
              <button
                type="button"
                class="pointer-events-auto absolute -left-[7px] top-1.5 size-3.5 cursor-ew-resize rounded-full bg-primary shadow ring-2 ring-card"
                title="Drag to scrub"
                aria-label="Drag playhead"
                onpointerdown={startDrag}
                onclick={(e) => e.stopPropagation()}
              ></button>
            </div>
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
    </div>
  {/if}
</div>
