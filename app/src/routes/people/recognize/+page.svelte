<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { toast } from "svelte-sonner";

  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Camera from "@lucide/svelte/icons/camera";
  import Upload from "@lucide/svelte/icons/upload";
  import ScanSearch from "@lucide/svelte/icons/scan-search";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import PointCloudAvatar from "$lib/components/point-cloud-avatar.svelte";

  import type { Device, PersonEmbedding, RecognizeDetail } from "$lib/types";

  let cameras = $state<Device[]>([]);
  let selectedIp = $state("");
  let loading = $state(true);
  let running = $state(false);

  let detail = $state<RecognizeDetail | null>(null);
  let matchFaces = $state<PersonEmbedding[]>([]);
  let fileEl = $state<HTMLInputElement | null>(null);

  function cameraName(ip: string): string {
    return cameras.find((c) => c.ip === ip)?.name ?? "Select a camera";
  }

  async function load() {
    loading = true;
    try {
      cameras = await invoke<Device[]>("list_devices");
      if (!selectedIp && cameras.length > 0) selectedIp = cameras[0].ip;
    } catch (err) {
      console.error("Failed to load cameras", err);
      toast.error("Failed to load cameras", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function run(image: string) {
    running = true;
    detail = null;
    matchFaces = [];
    try {
      const d = await invoke<RecognizeDetail>("recognize_face", { image });
      detail = d;
      // Pull the matched person's enrolled samples to show their point-cloud beside the target.
      if (d.match) {
        const faces = await invoke<PersonEmbedding[]>("list_embeddings", { personId: d.match.person_id });
        matchFaces = faces.filter((f) => f.approved);
        if (matchFaces.length === 0) matchFaces = faces;
      }
    } catch (err) {
      console.error("Recognition failed", err);
      toast.error("Recognition failed", { description: String(err) });
    } finally {
      running = false;
    }
  }

  async function runFromCamera() {
    if (!selectedIp) {
      toast.error("Pick a camera first.");
      return;
    }
    let dataUrl: string;
    try {
      dataUrl = await invoke<string>("capture_snapshot", { ip: selectedIp });
    } catch (err) {
      toast.error("Capture failed", { description: String(err) });
      return;
    }
    await run(dataUrl);
  }

  function onPickFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => run(reader.result as string);
    reader.readAsDataURL(file);
  }
</script>

<div class="mx-auto max-w-4xl space-y-6">
  <!-- top bar -->
  <div class="flex items-center gap-3">
    <Button variant="ghost" size="icon" title="Back to People" onclick={() => goto("/people")}>
      <ArrowLeft class="size-4" />
    </Button>
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Recognition</h1>
      <p class="text-sm text-muted-foreground">
        Probe a frame or photo against the enrolled people. Nothing is stored — this only reads.
      </p>
    </div>
  </div>

  {#if loading}
    <Loader label="Loading…" />
  {:else}
    <!-- Source -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><ScanSearch class="size-4 text-muted-foreground" /> Probe</Card.Title>
        <Card.Description>Capture from a saved camera or upload a photo to recognise.</Card.Description>
      </Card.Header>
      <Card.Content class="flex flex-wrap items-end gap-3">
        {#if cameras.length > 0}
          <div class="space-y-2">
            <Label>Camera</Label>
            <Select.Root type="single" value={selectedIp} onValueChange={(v) => (selectedIp = v ?? "")}>
              <Select.Trigger class="w-56">{cameraName(selectedIp)}</Select.Trigger>
              <Select.Content>
                {#each cameras as c (c.ip)}
                  <Select.Item value={c.ip} label={c.name}>{c.name}</Select.Item>
                {/each}
              </Select.Content>
            </Select.Root>
          </div>
          <Button onclick={runFromCamera} disabled={running}>
            {#if running}<LoaderCircle class="size-4 animate-spin" /> Working…{:else}<Camera class="size-4" /> Recognise from camera{/if}
          </Button>
        {/if}
        <Button variant="outline" onclick={() => fileEl?.click()} disabled={running}>
          <Upload class="size-4" /> Recognise a photo
        </Button>
        <input bind:this={fileEl} type="file" accept="image/*" class="hidden" onchange={onPickFile} />
      </Card.Content>
    </Card.Root>

    <!-- Result: target vs recognised, as point clouds -->
    {#if detail}
      <Card.Root>
        <Card.Header>
          <Card.Title>Result</Card.Title>
          <Card.Description>The probe (target) compared with the person the system recognised.</Card.Description>
        </Card.Header>
        <Card.Content>
          <div class="flex flex-wrap items-center justify-center gap-6">
            <!-- target -->
            <div class="flex flex-col items-center gap-2">
              <PointCloudAvatar embedding={detail.query} size={240} dots={300} />
              <span class="text-sm font-medium">Target</span>
              <span class="text-xs text-muted-foreground">the probe image</span>
            </div>

            <ArrowRight class="size-6 shrink-0 text-muted-foreground" />

            <!-- recognised -->
            <div class="flex flex-col items-center gap-2">
              {#if detail.match && matchFaces.length > 0}
                <PointCloudAvatar embedding={matchFaces[0].embedding} size={240} dots={300} />
                <span class="text-sm font-medium">{detail.match.name}</span>
                <Badge variant="outline" class="border-primary/40 text-primary">{detail.match.confidence}% match</Badge>
              {:else}
                <div class="grid size-[240px] place-items-center rounded-lg border border-dashed border-border text-sm text-muted-foreground">
                  No match
                </div>
                <span class="text-sm font-medium text-muted-foreground">No one recognised</span>
              {/if}
            </div>
          </div>
        </Card.Content>
      </Card.Root>
    {/if}
  {/if}
</div>
