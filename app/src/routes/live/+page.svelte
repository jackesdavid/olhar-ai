<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { toast } from "svelte-sonner";

  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import VideoOff from "@lucide/svelte/icons/video-off";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";

  import type { Device, StreamInfo } from "$lib/types";

  let devices = $state<Device[]>([]);
  let streams = $state<StreamInfo[]>([]);
  let loading = $state(true);
  let rescanning = $state(false);
  let selectedIp = $state<string>("");

  // Devices that have a usable stream (rtsp_url validated → in `streams`).
  const streamByIp = $derived(new Map(streams.map((s) => [s.ip, s])));
  const playable = $derived(devices.filter((d) => streamByIp.has(d.ip)));
  // Devices without a validated rtsp_url — surface them as a hint.
  const unvalidated = $derived(devices.filter((d) => !streamByIp.has(d.ip)));

  const selected = $derived(playable.find((d) => d.ip === selectedIp) ?? null);
  const selectedStream = $derived(selectedIp ? streamByIp.get(selectedIp) ?? null : null);
  const selectedLabel = $derived(selected ? selected.name : "Select a camera");

  async function refresh(announce = false) {
    try {
      const [devs, strs] = await Promise.all([
        invoke<Device[]>("list_devices"),
        invoke<StreamInfo[]>("ensure_streams"),
      ]);
      devices = devs;
      streams = strs;

      // Keep / pick a sensible selection.
      if (!selectedIp || !strs.some((s) => s.ip === selectedIp)) {
        selectedIp = strs.length > 0 ? strs[0].ip : "";
      }

      if (announce) {
        if (strs.length === 0) {
          toast.info("No camera streams yet.", {
            description: "Validate a camera's credentials on the Devices page first.",
          });
        } else {
          toast.success(`${strs.length} stream${strs.length === 1 ? "" : "s"} ready.`);
        }
      }
    } catch (err) {
      console.error("Failed to load live streams", err);
      toast.error("Failed to load live streams", { description: String(err) });
    }
  }

  onMount(async () => {
    loading = true;
    await refresh(false);
    loading = false;
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
        Real-time camera video over WebRTC, served by the bundled go2rtc sidecar.
      </p>
    </div>
    <Button variant="outline" onclick={rescan} disabled={rescanning}>
      {#if rescanning}
        <LoaderCircle class="size-4 animate-spin" />
        Rescanning…
      {:else}
        <RefreshCw class="size-4" />
        Rescan streams
      {/if}
    </Button>
  </div>

  {#if loading}
    <p class="py-10 text-center text-sm text-muted-foreground">Loading streams…</p>
  {:else}
    <Card.Root>
      <Card.Content class="space-y-4 pt-6">
        {#if playable.length === 0}
          <div
            class="flex flex-col items-center gap-3 rounded-md border border-dashed border-border py-12 text-center"
          >
            <VideoOff class="size-8 text-muted-foreground" />
            <div>
              <p class="text-sm font-medium">No camera streams available</p>
              <p class="text-sm text-muted-foreground">
                Add a camera and validate its credentials on the
                <a href="/devices" class="underline underline-offset-4">Devices</a> page first —
                that stores its RTSP URL, which this screen needs.
              </p>
            </div>
          </div>
        {:else}
          <div class="space-y-2">
            <Label>Camera</Label>
            <Select.Root type="single" bind:value={selectedIp}>
              <Select.Trigger class="w-72">
                {selectedLabel}
              </Select.Trigger>
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

          <!-- 16:9 framed WebRTC player. go2rtc serves its player at
               http://localhost:1984/webrtc.html?src=<stream>; the iframe runs
               same-origin to localhost:1984. -->
          <div class="frame">
            {#if selectedStream}
              {#key selectedStream.name}
                <iframe
                  title="Live camera {selected?.name ?? ''}"
                  src={selectedStream.player_url}
                  allow="autoplay; fullscreen"
                ></iframe>
              {/key}
            {:else}
              <div class="flex h-full items-center justify-center text-sm text-muted-foreground">
                Select a camera to start streaming.
              </div>
            {/if}
          </div>

          {#if selectedStream}
            <p class="text-xs text-muted-foreground">
              Stream <code class="rounded bg-muted px-1 py-0.5">{selectedStream.name}</code> ·
              <a
                href={selectedStream.player_url}
                target="_blank"
                rel="noreferrer"
                class="underline underline-offset-4">open in browser</a
              >
            </p>
          {/if}
        {/if}

        {#if unvalidated.length > 0}
          <p class="text-xs text-muted-foreground">
            {unvalidated.length} saved camera{unvalidated.length === 1 ? "" : "s"} without a stream
            (no validated RTSP URL): {unvalidated.map((d) => d.name).join(", ")}. Validate
            {unvalidated.length === 1 ? "it" : "them"} on the Devices page first.
          </p>
        {/if}
      </Card.Content>
    </Card.Root>

    <details class="help text-sm text-muted-foreground">
      <summary class="cursor-pointer text-foreground/80">
        Not seeing video? Cross-OS WebRTC notes &amp; troubleshooting
      </summary>
      <ol class="mt-2 list-decimal space-y-1 pl-5">
        <li>
          Drop the go2rtc binary at <code class="rounded bg-muted px-1 py-0.5"
            >src-tauri/binaries/go2rtc</code
          > (see <code class="rounded bg-muted px-1 py-0.5">binaries/DOWNLOAD.md</code>). It is
          never committed.
        </li>
        <li>Validate each camera's credentials on the Devices page (stores its RTSP URL).</li>
        <li>
          Hit <strong>Rescan streams</strong> — the backend regenerates
          <code class="rounded bg-muted px-1 py-0.5">go2rtc.yaml</code> and restarts the sidecar.
        </li>
        <li>
          Confirm go2rtc is up: open <code class="rounded bg-muted px-1 py-0.5"
            >http://localhost:1984</code
          > in a browser — the stream should play there.
        </li>
      </ol>
      <p class="warn mt-2">
        Expected differences by OS webview (the cross-OS media go/no-go):
        <br />• <strong>macOS</strong> (WKWebView) — should work.
        <br />• <strong>Windows</strong> (WebView2) — works, but a clean box may lack proprietary
        H.264; go2rtc passthrough of H.264 can hit this. Verify on a fresh machine.
        <br />• <strong>Linux</strong> (WebKitGTK) — WebRTC is often
        <strong>off by default</strong>; this is the most likely failure and the key thing to
        prove.
      </p>
    </details>
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
  .help {
    line-height: 1.5;
  }
  .warn {
    border-left: 3px solid #d97706;
    padding-left: 0.75rem;
    line-height: 1.7;
  }
</style>
