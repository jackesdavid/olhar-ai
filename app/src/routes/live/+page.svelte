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
  function streamName(ip: string): string {
    return "cam_" + ip.replaceAll(".", "_");
  }

  // Cloud live frame URL through the gateway (live.view-gated; token in the query since
  // <img> can't send headers). `tick` busts the cache to refresh.
  let tick = $state(0);
  const cloudFrameUrl = $derived(
    selected && auth.token
      ? `${GATEWAY_BASE}/api/frame.jpeg?src=${streamName(selected.ip)}&token=${encodeURIComponent(auth.token)}&t=${tick}`
      : "",
  );

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
    // Cloud: refresh the live frame ~1/s.
    const timer = cloud ? setInterval(() => (tick = Date.now()), 1000) : undefined;
    return () => {
      if (timer) clearInterval(timer);
    };
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

          <div class="frame relative">
            {#if selected}
              <div class="pointer-events-none absolute left-3 top-3 z-10 flex items-center gap-1.5 rounded-full border border-primary/30 bg-background/70 px-2.5 py-1 text-[11px] font-medium tracking-wide text-primary backdrop-blur">
                <StatusDot state="online" /> LIVE
              </div>
              {#if cloud}
                <!-- gateway live frame (token-gated), refreshed ~1/s -->
                <img src={cloudFrameUrl} alt="Live from {selected.name}" class="h-full w-full object-contain" />
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
