<script lang="ts">
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Clapperboard from "@lucide/svelte/icons/clapperboard";
  import Download from "@lucide/svelte/icons/download";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";

  import { Button } from "$lib/components/ui/button";
  import * as Table from "$lib/components/ui/table";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import { recordings, type Recording } from "$lib/api";

  let loading = $state(true);
  let items = $state<Recording[]>([]);

  async function load() {
    loading = true;
    try {
      items = await recordings.list({ limit: 200 });
    } catch (e) {
      toast.error(String(e));
    } finally {
      loading = false;
    }
  }

  async function remove(id: number) {
    try {
      await recordings.del(id);
      items = items.filter((r) => r.id !== id);
      toast.success("Recording deleted");
    } catch (e) {
      toast.error(String(e));
    }
  }

  function fmtDuration(secs: number) {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return m > 0 ? `${m}m ${s}s` : `${s}s`;
  }

  function fmtSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function fmtDate(iso: string) {
    return new Date(iso).toLocaleString();
  }

  onMount(load);
</script>

<div class="flex flex-col gap-6 p-6">
  <div class="flex items-center justify-between">
    <div class="flex items-center gap-3">
      <Clapperboard class="h-6 w-6 text-emerald-400" />
      <h1 class="text-xl font-semibold">Recordings</h1>
    </div>
    <Button variant="outline" size="sm" onclick={load} disabled={loading}>
      <RefreshCw class="mr-2 h-4 w-4" />
      Refresh
    </Button>
  </div>

  {#if loading}
    <Loader />
  {:else if items.length === 0}
    <EmptyState
      title="No recordings yet"
      description="Recordings appear here once a workflow triggers the Start Recording node and a clip is finalised."
    />
  {:else}
    <div class="rounded-lg border border-white/10 overflow-hidden">
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <Table.Head>Camera</Table.Head>
            <Table.Head>Started</Table.Head>
            <Table.Head>Duration</Table.Head>
            <Table.Head>Size</Table.Head>
            <Table.Head class="w-24">Actions</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each items as rec (rec.id)}
            <Table.Row>
              <Table.Cell class="font-mono text-sm">{rec.camera_ip}</Table.Cell>
              <Table.Cell class="text-sm text-muted-foreground">{fmtDate(rec.started_at)}</Table.Cell>
              <Table.Cell class="text-sm">{fmtDuration(rec.duration_secs)}</Table.Cell>
              <Table.Cell class="text-sm text-muted-foreground">{fmtSize(rec.size_bytes)}</Table.Cell>
              <Table.Cell>
                <div class="flex gap-1">
                  <a
                    href={recordings.downloadUrl(rec.id)}
                    download="{rec.clip_id}.mp4"
                    data-testid="download-{rec.id}"
                  >
                    <Button variant="ghost" size="icon" title="Download">
                      <Download class="h-4 w-4" />
                    </Button>
                  </a>
                  <Button
                    variant="ghost"
                    size="icon"
                    title="Delete"
                    onclick={() => remove(rec.id)}
                    data-testid="delete-{rec.id}"
                  >
                    <Trash2 class="h-4 w-4 text-destructive" />
                  </Button>
                </div>
              </Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    </div>
  {/if}
</div>
