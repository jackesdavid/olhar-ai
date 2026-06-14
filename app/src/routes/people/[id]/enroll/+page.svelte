<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { data } from "$lib/data";
  import { toast } from "svelte-sonner";

  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Camera from "@lucide/svelte/icons/camera";
  import Upload from "@lucide/svelte/icons/upload";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Check from "@lucide/svelte/icons/check";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import PointCloudAvatar from "$lib/components/point-cloud-avatar.svelte";

  import type { Person, PersonEmbedding, Device } from "$lib/types";

  const id = $derived($page.params.id ?? "");

  let person = $state<Person | null>(null);
  let cameras = $state<Device[]>([]);
  let embeddings = $state<PersonEmbedding[]>([]);
  let loading = $state(true);

  let selectedIp = $state("");
  let approveOnCapture = $state(true);
  let busy = $state(false); // capture/upload/enroll in flight
  let fileEl = $state<HTMLInputElement | null>(null);

  const approvedCount = $derived(embeddings.filter((e) => e.approved).length);
  function cameraName(ip: string): string {
    return cameras.find((c) => c.ip === ip)?.name ?? "Select a camera";
  }

  async function loadEmbeddings() {
    embeddings = await data.listEmbeddings(id);
  }

  async function load() {
    loading = true;
    try {
      const [people, devices] = await Promise.all([
        data.listPeople(),
        data.listDevices(),
      ]);
      person = people.find((p) => p.id === id) ?? null;
      cameras = devices;
      if (!selectedIp && devices.length > 0) selectedIp = devices[0].ip;
      await loadEmbeddings();
    } catch (err) {
      console.error("Failed to load enrollment", err);
      toast.error("Failed to load", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(load);

  // Compute + store an embedding from an image (data URL). The photo is never stored —
  // the backend embeds it and keeps only the vector.
  async function enroll(image: string, source: string) {
    busy = true;
    try {
      await data.enrollFace(id, image, source, approveOnCapture);
      toast.success("Face enrolled.");
      await loadEmbeddings();
    } catch (err) {
      console.error("Enroll failed", err);
      toast.error("Enroll failed", { description: String(err) });
    } finally {
      busy = false;
    }
  }

  async function captureFromCamera() {
    if (!selectedIp) {
      toast.error("Pick a camera first.");
      return;
    }
    busy = true;
    try {
      const dataUrl = await data.captureSnapshot(selectedIp);
      await enroll(dataUrl, "camera");
    } catch (err) {
      console.error("Capture failed", err);
      toast.error("Capture failed", { description: String(err) });
      busy = false;
    }
  }

  function onPickFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => enroll(reader.result as string, "upload");
    reader.readAsDataURL(file);
  }

  async function approve(emb: PersonEmbedding) {
    try {
      await data.approveEmbedding(emb.id);
      await loadEmbeddings();
    } catch (err) {
      toast.error("Approve failed", { description: String(err) });
    }
  }
  async function remove(emb: PersonEmbedding) {
    try {
      await data.deleteEmbedding(emb.id);
      await loadEmbeddings();
    } catch (err) {
      toast.error("Remove failed", { description: String(err) });
    }
  }
</script>

<div class="mx-auto max-w-4xl space-y-6">
  <!-- top bar -->
  <div class="flex items-center gap-3">
    <Button variant="ghost" size="icon" title="Back to record" onclick={() => goto(`/people/${id}`)}>
      <ArrowLeft class="size-4" />
    </Button>
    <div class="min-w-0">
      <h1 class="truncate text-2xl font-semibold tracking-tight">
        Enroll — {person?.name ?? "Person"}
      </h1>
      <p class="text-sm text-muted-foreground">
        Capture several angles for stronger recognition. We store only the face embedding — never the
        photo.
      </p>
    </div>
  </div>

  {#if loading}
    <Loader label="Loading…" />
  {:else}
    <!-- Capture controls -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><Camera class="size-4 text-muted-foreground" /> Capture</Card.Title>
        <Card.Description>Grab a frame from a saved camera, or upload a photo as a fallback.</Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        {#if cameras.length === 0}
          <p class="rounded-md border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
            No cameras — add one in Devices, or upload a photo below.
          </p>
        {:else}
          <div class="flex flex-wrap items-end gap-3">
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
            <Button onclick={captureFromCamera} disabled={busy}>
              {#if busy}
                <LoaderCircle class="size-4 animate-spin" /> Working…
              {:else}
                <Camera class="size-4" /> Capture from camera
              {/if}
            </Button>
          </div>
        {/if}

        <div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-3">
          <label class="flex items-center gap-2 text-sm">
            <Switch checked={approveOnCapture} onCheckedChange={(v) => (approveOnCapture = v)} />
            <span class="flex items-center gap-1.5">
              <ShieldCheck class="size-3.5 text-muted-foreground" /> Approve on capture (consent given)
            </span>
          </label>
          <Button variant="outline" onclick={() => fileEl?.click()} disabled={busy}>
            <Upload class="size-4" /> Upload photo
          </Button>
          <input bind:this={fileEl} type="file" accept="image/*" class="hidden" onchange={onPickFile} />
        </div>
      </Card.Content>
    </Card.Root>

    <!-- Enrolled samples -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center justify-between gap-2">
          <span>Enrolled faces</span>
          <Badge variant="outline">{approvedCount} approved / {embeddings.length} total</Badge>
        </Card.Title>
        <Card.Description>
          Each sample is shown as a point-cloud fingerprint derived from its embedding — no photo is kept.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        {#if embeddings.length === 0}
          <EmptyState title="No faces enrolled" description="Capture from a camera or upload a photo above." />
        {:else}
          <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4">
            {#each embeddings as emb (emb.id)}
              <div class="space-y-2 rounded-xl border border-border p-3">
                <PointCloudAvatar embedding={emb.embedding} size={120} />
                <div class="flex items-center justify-between gap-2">
                  <Badge variant="outline" class="text-[10px] capitalize">{emb.source || "?"}</Badge>
                  {#if emb.approved}
                    <span class="flex items-center gap-1 text-[11px] text-primary"><Check class="size-3" /> Approved</span>
                  {:else}
                    <span class="text-[11px] text-amber-500">Pending</span>
                  {/if}
                </div>
                <div class="flex items-center gap-1">
                  {#if !emb.approved}
                    <Button variant="ghost" size="sm" class="h-7 flex-1 text-xs" onclick={() => approve(emb)}>
                      <Check class="size-3.5" /> Approve
                    </Button>
                  {/if}
                  <Button variant="ghost" size="sm" class="h-7 text-xs text-muted-foreground hover:text-destructive" onclick={() => remove(emb)}>
                    <Trash2 class="size-3.5" />
                  </Button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}
</div>
