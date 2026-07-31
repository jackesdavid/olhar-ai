<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { toast } from "svelte-sonner";

  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import Busy from "$lib/components/busy.svelte";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";
  import { Label } from "$lib/components/ui/label";

  import { data } from "$lib/data";
  import { GATEWAY_BASE } from "$lib/config";
  import { auth } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";
  import type { Device, StreamInfo } from "$lib/types";

  // Web-only: live frames always come from the edge via the streaming gateway.
  const cloud = true;

  const t = $derived(dict[lang()].live);

  let devices = $state<Device[]>([]);
  let streams = $state<StreamInfo[]>([]); // desktop only
  let loading = $state(true);
  let rescanning = $state(false);
  let selectedIp = $state<string>("");
  let hd = $state(false); // cloud: SD substream (default) vs HD main stream

  // live on the edge (never in the cloud), so we list all cameras — the frame loads if
  // the edge is serving it, blank if its credentials aren't set yet.
  const streamByIp = $derived(new Map(streams.map((s) => [s.ip, s])));
  const playable = $derived(cloud ? devices : devices.filter((d) => streamByIp.has(d.ip)));
  const unvalidated = $derived(cloud ? [] : devices.filter((d) => !streamByIp.has(d.ip)));

  const selected = $derived(playable.find((d) => d.ip === selectedIp) ?? null);
  const selectedStream = $derived(selectedIp ? streamByIp.get(selectedIp) ?? null : null);
  const selectedLabel = $derived(selected ? selected.name : t.selectCamera);

  // The edge serves a low-latency substream by default and an `_hd` main variant.
  function streamName(ip: string): string {
    return "cam_" + ip.replaceAll(".", "_") + (hd ? "_hd" : "");
  }

  // ---- Cloud live video: MSE over the gateway WebSocket (smooth H.264) -----------
  // The token rides the WS query (browsers can't set headers on a WebSocket, and this
  let videoEl = $state<HTMLVideoElement>();
  let ws: WebSocket | null = null;
  let mediaSource: MediaSource | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let bufQueue: ArrayBuffer[] = [];

  // ---- Honest playback state (card #89) -----------------------------------------
  // The badge must reflect what's actually on screen: LIVE only when the playhead is truly
  // advancing, our loader while connecting, a clear error (never a green LIVE over black).
  type Phase = "idle" | "connecting" | "live" | "stalled" | "error";
  let phase = $state<Phase>("idle");
  let errorMsg = $state("");
  let retries = 0; // stall auto-retries since we were last live
  const MAX_RETRIES = 3;
  let watchdog: ReturnType<typeof setTimeout> | null = null; // no-first-frame timeout
  let stallTimer: ReturnType<typeof setTimeout> | null = null; // frames-stopped-while-live timeout
  let lastTime = 0; // last observed playhead, to detect real progress

  function clearTimers() {
    if (watchdog) { clearTimeout(watchdog); watchdog = null; }
    if (stallTimer) { clearTimeout(stallTimer); stallTimer = null; }
  }

  function fail(msg: string) {
    clearTimers();
    errorMsg = msg;
    phase = "error";
  }

  /** Why a connect might have failed — codec-specific when we're on HD, else generic. */
  function connectFailMsg(): string {
    return hd ? t.errHdCodec : t.errConnect;
  }

  // An advancing playhead is the ONLY honest signal that frames are rendering → truly live.
  function onProgress() {
    if (!videoEl) return;
    if (videoEl.currentTime > lastTime + 0.01) {
      lastTime = videoEl.currentTime;
      if (phase !== "live") {
        phase = "live";
        retries = 0;
        errorMsg = "";
      }
      if (watchdog) { clearTimeout(watchdog); watchdog = null; }
      armStall();
    }
  }

  // Was live, then the playhead stopped advancing → treat as a stall and try to recover.
  function armStall() {
    if (stallTimer) clearTimeout(stallTimer);
    stallTimer = setTimeout(() => {
      if (phase === "live") {
        phase = "stalled";
        autoRetry();
      }
    }, 4000);
  }

  function autoRetry() {
    if (retries >= MAX_RETRIES) {
      fail(t.errKeepsDropping);
      return;
    }
    retries += 1;
    const ip = selected?.ip;
    if (ip) startStream(ip, true);
  }

  function manualRetry() {
    retries = 0;
    const ip = selected?.ip;
    if (ip) startStream(ip);
  }

  // ---- badge + overlay specs (house style: named helpers, no markup ternaries) ---
  function badgeSpec(): { text: string; dot: "online" | "warn" | "danger"; cls: string } | null {
    switch (phase) {
      case "live":
        return { text: t.badgeLive, dot: "online", cls: "border-primary/30 bg-hud text-primary" };
      case "connecting":
        return { text: t.badgeConnecting, dot: "warn", cls: "border-border bg-hud text-muted-foreground" };
      case "stalled":
        return { text: t.badgeReconnecting, dot: "warn", cls: "border-border bg-hud text-muted-foreground" };
      case "error":
        return { text: t.badgeNoSignal, dot: "danger", cls: "border-red-400/30 bg-hud text-red-400" };
      default:
        return null;
    }
  }
  function showConnectingOverlay(): boolean {
    return phase === "connecting" || phase === "stalled";
  }
  function connectingLabel(): string {
    return phase === "stalled" ? t.badgeReconnecting : t.connectingToCamera;
  }

  function probeCodecs(): string {
    const v = ["avc1.640029", "avc1.64002A", "avc1.640033", "avc1.42E01E", "hvc1.1.6.L153.B0"];
    const a = ["mp4a.40.2", "mp4a.40.5", "flac", "opus"];
    const ok = (mime: string) => "MediaSource" in window && MediaSource.isTypeSupported(mime);
    const out = [
      ...v.filter((c) => ok(`video/mp4; codecs="${c}"`)),
      ...a.filter((c) => ok(`audio/mp4; codecs="${c}"`)),
    ];
    return out.join(",");
  }

  function flushBuf() {
    if (!sourceBuffer || sourceBuffer.updating || bufQueue.length === 0) return;
    try {
      sourceBuffer.appendBuffer(bufQueue.shift()!);
    } catch {
      bufQueue = [];
    }
  }

  // starts OUTSIDE the buffered range (→ black). Seek into the range / to the live edge,
  // trim old data, and start playback.
  function onUpdateEnd() {
    flushBuf();
    if (!sourceBuffer || !videoEl || sourceBuffer.buffered.length === 0) return;
    const b = sourceBuffer.buffered;
    const start = b.start(0);
    const end = b.end(b.length - 1);
    if (videoEl.currentTime < start || videoEl.currentTime > end) {
      try { videoEl.currentTime = Math.max(start, end - 0.5); } catch { /* ignore */ }
    } else if (end - videoEl.currentTime > 1.5) {
      try { videoEl.currentTime = end - 0.3; } catch { /* ignore */ } // keep near live
    }
    if (end - start > 12 && !sourceBuffer.updating) {
      try { sourceBuffer.remove(start, end - 8); } catch { /* ignore */ }
    }
    if (videoEl.paused) videoEl.play().catch(() => {});
  }

  function stopStream() {
    clearTimers();
    if (ws) {
      ws.onmessage = ws.onclose = ws.onerror = null;
      try { ws.close(); } catch { /* ignore */ }
      ws = null;
    }
    sourceBuffer = null;
    bufQueue = [];
    mediaSource = null;
    lastTime = 0;
    phase = "idle";
    if (videoEl) {
      videoEl.removeEventListener("timeupdate", onProgress);
      videoEl.removeAttribute("src");
      try { videoEl.load(); } catch { /* ignore */ }
    }
  }

  function startStream(ip: string, reconnecting = false) {
    stopStream();
    if (!videoEl || !auth.token || !("MediaSource" in window)) {
      fail(t.errUnsupportedBrowser);
      return;
    }
    // Nothing we send would decode → don't dangle on a black screen waiting for a frame.
    if (probeCodecs() === "") {
      fail(t.errNoCodecs);
      return;
    }
    phase = reconnecting ? "stalled" : "connecting";
    errorMsg = "";
    lastTime = 0;
    videoEl.addEventListener("timeupdate", onProgress);
    // First-frame watchdog: if the playhead never advances, surface a real error instead of black.
    watchdog = setTimeout(() => {
      if (phase !== "live") fail(connectFailMsg());
    }, 10000);
    const wsBase = GATEWAY_BASE.replace(/^http/, "ws");
    const url = `${wsBase}/api/ws?src=${streamName(ip)}&token=${encodeURIComponent(auth.token)}`;
    const socket = new WebSocket(url);
    socket.binaryType = "arraybuffer";
    ws = socket;

    const ms = new MediaSource();
    mediaSource = ms;
    videoEl.src = URL.createObjectURL(ms);

    // The handshake must be sent only once BOTH the WebSocket is open AND MediaSource is
    // open — sourceopen often fires before the WS finishes connecting (send-on-CONNECTING
    // would throw and we'd get no data → black video).
    let wsReady = false;
    let msReady = false;
    let sent = false;
    const maybeHandshake = () => {
      if (wsReady && msReady && !sent && ws === socket) {
        sent = true;
        try { socket.send(JSON.stringify({ type: "mse", value: probeCodecs() })); } catch { /* ignore */ }
      }
    };

    socket.onopen = () => { wsReady = true; maybeHandshake(); };
    ms.addEventListener("sourceopen", () => { msReady = true; maybeHandshake(); });
    socket.onmessage = (ev) => {
      if (ws !== socket) return;
      if (typeof ev.data === "string") {
        const msg = JSON.parse(ev.data);
        if (msg.type === "mse" && mediaSource?.readyState === "open" && !sourceBuffer) {
          try {
            sourceBuffer = mediaSource.addSourceBuffer(msg.value);
            sourceBuffer.mode = "segments";
            sourceBuffer.addEventListener("updateend", onUpdateEnd);
          } catch (e) {
            console.error("addSourceBuffer failed", e);
            fail(connectFailMsg()); // codec the browser can't decode → honest error, not black
          }
        }
      } else {
        bufQueue.push(ev.data as ArrayBuffer);
        flushBuf();
      }
    };
    // An unexpected close/error (stopStream nulls these first, so this only fires for real drops):
    // recover if we had video, else surface why nothing started.
    socket.onclose = () => {
      if (ws !== socket) return; // superseded by a newer stream
      if (phase === "live" || phase === "stalled") {
        phase = "stalled";
        autoRetry();
      } else {
        fail(t.errClosedEarly);
      }
    };
    socket.onerror = () => {}; // onclose will follow with the recovery/fail decision
  }

  // (Re)start the stream whenever the selected camera — or the HD toggle — changes (cloud only).
  $effect(() => {
    if (!cloud) return;
    const ip = selected?.ip;
    hd; // track: re-subscribe on quality change
    if (ip && videoEl) startStream(ip);
    else stopStream();
  });

  async function refresh(announce = false, prefer = "") {
    try {
      devices = await data.listDevices();
      streams = cloud ? [] : await data.ensureStreams();
      const ok = (ip: string) => (cloud ? devices.some((d) => d.ip === ip) : streams.some((s) => s.ip === ip));
      if (prefer && ok(prefer)) selectedIp = prefer;
      if (!selectedIp || !ok(selectedIp)) selectedIp = playable.length > 0 ? playable[0].ip : "";
      if (announce) {
        const n = playable.length;
        if (n === 0) toast.info(t.noStreamsTitle, { description: t.noStreamsHint });
        else toast.success(t.streamsReady(n));
      }
    } catch (err) {
      console.error("Failed to load live streams", err);
      toast.error(t.loadFailed, { description: String(err) });
    }
  }

  onMount(() => {
    loading = true;
    const cam = $page.url.searchParams.get("cam") ?? "";
    refresh(false, cam).finally(() => (loading = false));
    return () => stopStream();
  });

  async function rescan() {
    rescanning = true;
    try {
      await refresh(true);
    } finally {
      rescanning = false;
    }
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{t.title}</h1>
      <p class="text-sm text-muted-foreground">
        {cloud ? t.descriptionCloud : t.descriptionLocal}
      </p>
    </div>
    <Button variant="outline" onclick={rescan} disabled={rescanning}>
      {#if rescanning}<Busy /> {t.refreshing}{:else}<RefreshCw class="size-4" /> {t.refresh}{/if}
    </Button>
  </div>

  {#if loading}
    <Loader label={t.loadingStreams} />
  {:else}
    <Card.Root>
      <Card.Content class="space-y-4 pt-6">
        {#if playable.length === 0}
          <EmptyState
            title={t.emptyTitle}
            description={t.emptyDescription}
          >
            <Button href="/settings/devices" variant="outline">{t.goToDevices}</Button>
          </EmptyState>
        {:else}
          <div class="flex flex-wrap items-end justify-between gap-4">
            <div class="space-y-2">
              <Label>{t.camera}</Label>
              <Select.Root type="single" bind:value={selectedIp}>
                <Select.Trigger class="w-72">{selectedLabel}</Select.Trigger>
                <Select.Content>
                  {#each playable as d (d.ip)}
                    <Select.Item value={d.ip} label={d.name}>
                      {d.name}
                      <span class="ml-2 text-xs text-muted-foreground tabular-nums">{d.ip}</span>
                    </Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
            </div>

            {#if cloud && selected}
              <!-- Quality: SD = low-latency substream (default), HD = main stream -->
              <div class="inline-flex items-center rounded-full border border-border bg-muted/40 p-0.5 text-xs font-medium">
                <button
                  type="button"
                  class="rounded-full px-3 py-1 transition-colors {!hd ? 'bg-primary/15 text-primary' : 'text-muted-foreground hover:text-foreground'}"
                  onclick={() => (hd = false)}
                >SD</button>
                <button
                  type="button"
                  class="rounded-full px-3 py-1 transition-colors {hd ? 'bg-primary/15 text-primary' : 'text-muted-foreground hover:text-foreground'}"
                  onclick={() => (hd = true)}
                >HD</button>
              </div>
            {/if}
          </div>

          <div class="frame relative">
            {#if selected}
              {#if cloud}
                {@const b = badgeSpec()}
                <!-- Badge tells the truth: LIVE only when the playhead is advancing (see badgeSpec) -->
                {#if b}
                  <div class="pointer-events-none absolute left-3 top-3 z-10 flex items-center gap-1.5 rounded-full border {b.cls} px-2.5 py-1 text-[11px] font-medium tracking-wide backdrop-blur">
                    <StatusDot state={b.dot} /> {b.text}
                  </div>
                {/if}
                <!-- MSE video over the gateway WS (token-gated, smooth H.264) -->
                <video bind:this={videoEl} class="h-full w-full bg-video-well object-contain" autoplay muted playsinline></video>
                {#if showConnectingOverlay()}
                  <div class="absolute inset-0 z-10 flex items-center justify-center bg-black/60 backdrop-blur-sm">
                    <Loader label={connectingLabel()} />
                  </div>
                {:else if phase === "error"}
                  <div class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-black/80 p-6 text-center backdrop-blur-sm">
                    <WifiOff class="size-8 text-red-400" />
                    <p class="max-w-sm text-sm text-white/80">{errorMsg}</p>
                    <div class="flex items-center gap-2">
                      <Button size="sm" variant="outline" onclick={manualRetry}>
                        <RefreshCw class="size-4" /> {t.retry}
                      </Button>
                      {#if hd}
                        <Button size="sm" variant="ghost" class="text-white/80 hover:text-white" onclick={() => (hd = false)}>
                          {t.trySd}
                        </Button>
                      {/if}
                    </div>
                  </div>
                {/if}
              {:else if selectedStream}
                <div class="pointer-events-none absolute left-3 top-3 z-10 flex items-center gap-1.5 rounded-full border border-primary/30 bg-hud px-2.5 py-1 text-[11px] font-medium tracking-wide text-primary backdrop-blur">
                  <StatusDot state="online" /> {t.badgeLive}
                </div>
                {#key selectedStream.name}
                  <iframe title={t.liveCameraFrame(selected.name)} src={selectedStream.player_url} allow="autoplay; fullscreen"></iframe>
                {/key}
              {/if}
            {:else}
              <div class="flex h-full items-center justify-center text-sm text-muted-foreground">
                {t.selectToStart}
              </div>
            {/if}
          </div>
        {/if}

        {#if unvalidated.length > 0}
          <p class="text-xs text-muted-foreground">
            {t.unvalidated(unvalidated.length, unvalidated.map((d) => d.name).join(", "))}
          </p>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<style>
  .frame {
    aspect-ratio: 16 / 9;
    width: 100%;
    border: 1px solid hsl(var(--border));
    border-radius: 0.75rem;
    overflow: hidden;
    background: #000;
  }
  .frame iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }
</style>
