<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Plug from "@lucide/svelte/icons/plug";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { onMount } from "svelte";
  import { data } from "$lib/data";
  import { toast } from "svelte-sonner";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { Input } from "$lib/components/ui/input";
  import type { TuyaDevice } from "$lib/types";

  let configured = $state<boolean | null>(null);
  let devices = $state<TuyaDevice[]>([]);
  let loading = $state(true);
  let syncing = $state(false);
  // Per-device UI state: which rows are expanded, the in-progress label edits, and the saving flag.
  let expanded = $state<Record<string, boolean>>({});
  let edits = $state<Record<string, Record<string, string>>>({});
  let savingLabels = $state<string | null>(null);

  onMount(reload);

  type Fn = { code: string; type: string };

  function functionsOf(d: TuyaDevice): Fn[] {
    if (!d.functions_json) return [];
    try {
      const arr = JSON.parse(d.functions_json);
      return Array.isArray(arr) ? arr.map((f) => ({ code: f.code, type: f.type })) : [];
    } catch {
      return [];
    }
  }
  function labelsOf(d: TuyaDevice): Record<string, string> {
    if (!d.function_labels_json) return {};
    try {
      const o = JSON.parse(d.function_labels_json);
      return o && typeof o === "object" ? o : {};
    } catch {
      return {};
    }
  }
  function toggle(d: TuyaDevice) {
    const open = !expanded[d.device_id];
    expanded = { ...expanded, [d.device_id]: open };
    // Seed the edit buffer from the saved labels when opening.
    if (open && !edits[d.device_id]) edits = { ...edits, [d.device_id]: { ...labelsOf(d) } };
  }
  function editLabel(deviceId: string, code: string, value: string) {
    edits = { ...edits, [deviceId]: { ...(edits[deviceId] ?? {}), [code]: value } };
  }

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

  async function saveLabels(d: TuyaDevice) {
    savingLabels = d.device_id;
    try {
      await data.setTuyaFunctionLabels(d.device_id, edits[d.device_id] ?? {});
      toast.success("Labels saved");
      await reload();
    } catch (err) {
      console.error("Save labels failed", err);
      toast.error("Couldn't save labels", { description: String(err) });
    } finally {
      savingLabels = null;
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
            account. Name each control below so it reads clearly in a workflow. Use the
            <strong>Control Tuya Device</strong> and <strong>Read Tuya Status</strong> nodes.
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
            {@const fns = functionsOf(d)}
            <li class="py-2.5">
              <button
                type="button"
                class="interactive flex w-full items-center justify-between gap-3 text-left"
                onclick={() => toggle(d)}
              >
                <div class="flex min-w-0 items-center gap-2">
                  <ChevronRight class="size-4 shrink-0 text-muted-foreground transition-transform {expanded[d.device_id] ? 'rotate-90' : ''}" />
                  <div class="min-w-0">
                    <p class="truncate text-sm font-medium">{d.name}</p>
                    <p class="truncate text-xs text-muted-foreground">
                      {d.category ?? "device"} · {fns.length} control{fns.length === 1 ? "" : "s"}
                    </p>
                  </div>
                </div>
                <Badge variant={d.online ? "secondary" : "outline"} class="shrink-0">
                  {d.online ? "Online" : "Offline"}
                </Badge>
              </button>

              {#if expanded[d.device_id]}
                <div class="mt-3 space-y-2 pl-6">
                  {#if fns.length === 0}
                    <p class="text-xs text-muted-foreground">No controls reported — try Refresh devices.</p>
                  {:else}
                    {#each fns as f (f.code)}
                      <div class="flex items-center gap-3">
                        <div class="w-40 shrink-0">
                          <p class="truncate font-mono text-xs">{f.code}</p>
                          <p class="text-[10px] text-muted-foreground">{f.type}</p>
                        </div>
                        <Input
                          class="h-8"
                          placeholder="Friendly name (e.g. Luz da entrada)"
                          value={edits[d.device_id]?.[f.code] ?? ""}
                          oninput={(e) => editLabel(d.device_id, f.code, e.currentTarget.value)}
                        />
                      </div>
                    {/each}
                    <div class="flex justify-end pt-1">
                      <Button size="sm" onclick={() => saveLabels(d)} disabled={savingLabels === d.device_id}>
                        {savingLabels === d.device_id ? "Saving…" : "Save names"}
                      </Button>
                    </div>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
