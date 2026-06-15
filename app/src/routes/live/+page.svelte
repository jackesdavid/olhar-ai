<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { toast } from "svelte-sonner";

  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";
  import { Label } from "$lib/components/ui/label";

  import { data, isTauri } from "$lib/data";
  import { GATEWAY_BASE } from "$lib/config";
  import { auth } from "$lib/auth.svelte";
  import type { Device, StreamInfo } from "$lib/types";

  const cloud = !isTauri();

  let devices = $state<Device[]>([]);
  let streams = $state<StreamInfo[]>([]); // desktop only
  let loading = $state(true);
  let rescanning = $state(false);
  let selectedIp = $state<string>("");
  let hd = $state(false); // cloud: SD substream (default) vs HD main stream

  // Desktop: a camera is playable once go2rtc has a validated stream. Cloud: credentials
  // live on the edge (never in the cloud), so we list all cameras — the frame loads if
  // the edge is serving it, blank if its credentials aren't set yet.
  const streamByIp = $derived(new Map(streams.map((s) => [s.ip, s])));
  const playable = $derived(cloud ? devices : devices.filter((d) => streamByIp.has(d.ip)));
  const unvalidated = $derived(cloud ? [] : devices.filter((d) => !streamByIp.has(d.ip)));

  const selected = $derived(playable.find((d) => d.ip === selectedIp) ?? null);
  const selectedStream = $derived(selectedIp ? streamByIp.get(selectedIp) ?? null : null);
  const selectedLabel = $derived(selected ? selected.name : "Select a camera");

  // go2rtc stream name for an IP (matches the edge: cam_<ip with dots→underscores>).
  // The edge serves a low-latency substream by default and an `_hd` main variant.
  function streamName(ip: string): string {
    return "cam_" + ip.replaceAll(".", "_") + (hd ? "_hd" : "");
  }

  // ---- Cloud live video: MSE over the gateway WebSocket (smooth H.264) -----------
  // The token rides the WS query (browsers can't set headers on a WebSocket, and this
  // also avoids third-party cookies). go2rtc speaks fMP4-over-WS; we feed MediaSource.
  let videoEl = $state<HTMLVideoElement>();
  let ws: WebSocket | null = null;
  let mediaSource: MediaSource | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let bufQueue: ArrayBuffer[] = [];

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

  // After each append: go2rtc's fMP4 timestamps aren't zero-based, so the playhead
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
    if (ws) {
      ws.onmessage = ws.onclose = ws.onerror = null;
      try { ws.close(); } catch { /* ignore */ }
      ws = null;
    }
    sourceBuffer = null;
    bufQueue = [];
    mediaSource = null;
    if (videoEl) {
      videoEl.removeAttribute("src");
      try { videoEl.load(); } catch { /* ignore */ }
    }
  }

  function startStream(ip: string) {
    stopStream();
    if (!videoEl || !auth.token || !("MediaSource" in window)) return;
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
          }
        }
      } else {
        bufQueue.push(ev.data as ArrayBuffer);
        flushBuf();
      }
    };
    socket.onerror = () => {};
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
        if (n === 0) toast.info("No camera streams yet.", { description: "Add a camera with credentials on the Devices page." });
        else toast.success(`${n} stream${n === 1 ? "" : "s"} ready.`);
      }
    } catch (err) {
      console.error("Failed to load live streams", err);
      toast.error("Failed to load live streams", { description: String(err) });
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
      <h1 class="text-2xl font-semibold tracking-tight">Live</h1>
      <p class="text-sm text-muted-foreground">
        {cloud
          ? "Live camera feeds over your private edge link — never a public stream."
          : "Real-time camera video over WebRTC, served by the bundled go2rtc sidecar."}
      </p>
    </div>
    <Button variant="outline" onclick={rescan} disabled={rescanning}>
      {#if rescanning}<LoaderCircle class="size-4 animate-spin" /> Refreshing…{:else}<RefreshCw class="size-4" /> Refresh{/if}
    </Button>
  </div>

  {#if loading}
    <Loader label="Loading streams…" />
  {:else}
    <Card.Root>
      <Card.Content class="space-y-4 pt-6">
        {#if playable.length === 0}
          <EmptyState
            title="No camera streams yet"
            description="Add a camera and set its credentials on the Devices page — your edge resolves the stream automatically."
          >
            <Button href="/devices" variant="outline">Go to Devices</Button>
          </EmptyState>
        {:else}
          <div class="flex flex-wrap items-end justify-between gap-4">
            <div class="space-y-2">
              <Label>Camera</Label>
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
              <div class="pointer-events-none absolute left-3 top-3 z-10 flex items-center gap-1.5 rounded-full border border-primary/30 bg-background/70 px-2.5 py-1 text-[11px] font-medium tracking-wide text-primary backdrop-blur">
                <StatusDot state="online" /> LIVE
              </div>
              {#if cloud}
                <!-- MSE video over the gateway WS (token-gated, smooth H.264) -->
                <video bind:this={videoEl} class="h-full w-full bg-black object-contain" autoplay muted playsinline></video>
              {:else if selectedStream}
                {#key selectedStream.name}
                  <iframe title="Live camera {selected.name}" src={selectedStream.player_url} allow="autoplay; fullscreen"></iframe>
                {/key}
              {/if}
            {:else}
              <div class="flex h-full items-center justify-center text-sm text-muted-foreground">
                Select a camera to start streaming.
              </div>
            {/if}
          </div>
        {/if}

        {#if unvalidated.length > 0}
          <p class="text-xs text-muted-foreground">
            {unvalidated.length} saved camera{unvalidated.length === 1 ? "" : "s"} without a stream yet
            ({unvalidated.map((d) => d.name).join(", ")}). Set
            {unvalidated.length === 1 ? "its" : "their"} credentials on the Devices page.
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
