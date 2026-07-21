<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Plug from "@lucide/svelte/icons/plug";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { onMount } from "svelte";
  import { data } from "$lib/data";
  import { toast } from "svelte-sonner";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import type { TuyaDevice } from "$lib/types";

  let configured = $state<boolean | null>(null);
  let devices = $state<TuyaDevice[]>([]);
  let loading = $state(true);
  let syncing = $state(false);

  onMount(reload);

  async function reload() {
    loading = true;
    try {
      const [prov, list] = await Promise.all([
        data.getTuyaProvider().catch(() => ({ configured: false })),
        data.listTuyaDevices().catch(() => []),
      ]);
      configured = prov.configured;
      devices = list;
    } catch (err) {
      console.error("Failed to load Tuya integration", err);
      toast.error("Couldn't load Tuya devices", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  async function sync() {
    syncing = true;
    try {
      const { synced } = await data.syncTuyaDevices();
      toast.success(`Synced ${synced} Tuya device${synced === 1 ? "" : "s"}`);
      await reload();
    } catch (err) {
      console.error("Tuya sync failed", err);
      toast.error("Tuya sync failed", { description: String(err) });
    } finally {
      syncing = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Integrations"
    description="Third-party services Sentinel can act on from workflows."
    icon={Plug}
  />

  <Card.Root>
    <Card.Header>
      <div class="flex items-start justify-between gap-4">
        <div class="space-y-1">
          <Card.Title class="flex items-center gap-2">
            Tuya
            {#if configured === true}
              <Badge variant="secondary">Connected</Badge>
            {:else if configured === false}
              <Badge variant="outline">Not configured</Badge>
            {/if}
          </Card.Title>
          <Card.Description>
            Control smart-home devices (switches, plugs, lights) linked to your Tuya / Smart Life
            account. Use the <strong>Control Tuya Device</strong> and <strong>Read Tuya Status</strong>
            nodes in a workflow.
          </Card.Description>
        </div>
        <Button variant="outline" onclick={sync} disabled={syncing || loading || configured === false}>
          <RefreshCw class="mr-2 size-4 {syncing ? 'animate-spin' : ''}" />
          {syncing ? "Syncing…" : "Refresh devices"}
        </Button>
      </div>
    </Card.Header>
    <Card.Content>
      {#if configured === false}
        <p class="rounded border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
          Tuya isn't set up on the server yet. Once the operator adds the project credentials,
          link your account and press <em>Refresh devices</em>.
        </p>
      {:else if loading}
        <p class="text-sm text-muted-foreground">Loading…</p>
      {:else if devices.length === 0}
        <p class="rounded border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
          No devices yet. Link your Tuya / Smart Life account to the project, then press
          <em>Refresh devices</em>.
        </p>
      {:else}
        <ul class="divide-y divide-border">
          {#each devices as d (d.device_id)}
            <li class="flex items-center justify-between gap-3 py-2.5">
              <div class="min-w-0">
                <p class="truncate text-sm font-medium">{d.name}</p>
                <p class="truncate text-xs text-muted-foreground">
                  {d.category ?? "device"} · <span class="font-mono">{d.device_id}</span>
                </p>
              </div>
              <Badge variant={d.online ? "secondary" : "outline"} class="shrink-0">
                {d.online ? "Online" : "Offline"}
              </Badge>
            </li>
          {/each}
        </ul>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
