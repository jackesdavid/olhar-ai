<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Plug from "@lucide/svelte/icons/plug";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { onMount } from "svelte";
  import { data } from "$lib/data";
  import { toast } from "svelte-sonner";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import type { TuyaDevice } from "$lib/types";

  type Source = "tenant" | "operator" | "none";
  let source = $state<Source | null>(null);
  let devices = $state<TuyaDevice[]>([]);
  let loading = $state(true);
  let syncing = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let edits = $state<Record<string, Record<string, string>>>({});
  let savingLabels = $state<string | null>(null);

  // Connect-your-account form.
  const DATA_CENTERS = [
    { label: "Western America (US) — most Brazil/Americas accounts", url: "https://openapi.tuyaus.com" },
    { label: "Central Europe (EU)", url: "https://openapi.tuyaeu.com" },
    { label: "China", url: "https://openapi.tuyacn.com" },
    { label: "India", url: "https://openapi.tuyain.com" },
  ];
  let accessId = $state("");
  let accessKey = $state("");
  let endpoint = $state(DATA_CENTERS[0].url);
  let connecting = $state(false);
  let showGuide = $state(true);
  const dcLabel = $derived(DATA_CENTERS.find((d) => d.url === endpoint)?.label ?? "Data center");

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
    if (open && !edits[d.device_id]) edits = { ...edits, [d.device_id]: { ...labelsOf(d) } };
  }
  function editLabel(deviceId: string, code: string, value: string) {
    edits = { ...edits, [deviceId]: { ...(edits[deviceId] ?? {}), [code]: value } };
  }

  async function reload() {
    loading = true;
    try {
      const prov = await data.getTuyaProvider().catch(() => ({ configured: false, source: "none" as Source }));
      source = prov.source;
      devices = prov.configured ? await data.listTuyaDevices().catch(() => []) : [];
    } catch (err) {
      console.error("Failed to load Tuya integration", err);
      toast.error("Couldn't load Tuya", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  async function connect() {
    if (!accessId.trim() || !accessKey.trim()) {
      toast.error("Enter your Access ID and Access Secret");
      return;
    }
    connecting = true;
    try {
      await data.setTuyaConfig(accessId.trim(), accessKey.trim(), endpoint);
      toast.success("Tuya connected");
      accessKey = ""; // don't keep the secret around in the field
      await reload();
      await sync();
    } catch (err) {
      console.error("Tuya connect failed", err);
      toast.error("Couldn't connect Tuya", { description: String(err) });
    } finally {
      connecting = false;
    }
  }

  async function disconnect() {
    try {
      await data.deleteTuyaConfig();
      toast.success("Tuya disconnected");
      devices = [];
      await reload();
    } catch (err) {
      toast.error("Couldn't disconnect", { description: String(err) });
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
      toast.success("Names saved");
      await reload();
    } catch (err) {
      toast.error("Couldn't save names", { description: String(err) });
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
            {#if source === "tenant"}
              <Badge variant="secondary">Connected</Badge>
            {:else if source === "operator"}
              <Badge variant="secondary">Connected (managed)</Badge>
            {:else if source === "none"}
              <Badge variant="outline">Not connected</Badge>
            {/if}
          </Card.Title>
          <Card.Description>
            Control smart-home devices (switches, plugs, lights) from your Tuya / Smart Life account.
            Name each control so it reads clearly in a workflow, then use the
            <strong>Control Tuya Device</strong> and <strong>Read Tuya Status</strong> nodes.
          </Card.Description>
        </div>
        {#if source && source !== "none"}
          <Button variant="outline" onclick={sync} disabled={syncing || loading}>
            <RefreshCw class="mr-2 size-4 {syncing ? 'animate-spin' : ''}" />
            {syncing ? "Syncing…" : "Refresh devices"}
          </Button>
        {/if}
      </div>
    </Card.Header>
    <Card.Content class="space-y-4">
      {#if loading || source === null}
        <p class="text-sm text-muted-foreground">Loading…</p>

      <!-- ===== Connect flow (not connected) ===== -->
      {:else if source === "none"}
        <!-- Guided walkthrough: bring your own Tuya project (like Home Assistant / LocalTuya). -->
        <div class="rounded-lg border border-border">
          <button
            type="button"
            class="interactive flex w-full items-center gap-2 px-3 py-2.5 text-left text-sm font-medium"
            onclick={() => (showGuide = !showGuide)}
          >
            <ChevronRight class="size-4 text-muted-foreground transition-transform {showGuide ? 'rotate-90' : ''}" />
            How to connect your Tuya account (one-time, ~5 min)
          </button>
          {#if showGuide}
            <ol class="list-decimal space-y-2 border-t border-border px-6 py-3 text-sm text-muted-foreground">
              <li>
                Create a free developer account at
                <a href="https://iot.tuya.com" target="_blank" rel="noopener" class="inline-flex items-center gap-0.5 font-medium text-foreground underline">iot.tuya.com <ExternalLink class="size-3" /></a>.
              </li>
              <li><strong>Cloud → Development → Create Cloud Project.</strong> Development method: <strong>Smart Home</strong>. Pick the <strong>Data Center</strong> for your region (Brazil = Western America).</li>
              <li>Subscribe the APIs (free trial): <strong>IoT Core</strong>, <strong>Authorization</strong>, and <strong>Smart Home Basic Service</strong> (Service API → Go to Authorize).</li>
              <li><strong>Devices → Link App Account → Add App Account</strong> → scan the QR with your <strong>Smart Life</strong> app (Me → scan) and confirm. Your devices are now visible to the project.</li>
              <li>Open the project's <strong>Overview</strong> tab → copy the <strong>Access ID</strong> and <strong>Access Secret</strong> → paste them below.</li>
            </ol>
          {/if}
        </div>

        <div class="space-y-3">
          <div class="space-y-1.5">
            <Label for="tuya-dc">Data center</Label>
            <Select.Root type="single" bind:value={endpoint}>
              <Select.Trigger id="tuya-dc" class="w-full">{dcLabel}</Select.Trigger>
              <Select.Content>
                {#each DATA_CENTERS as dc (dc.url)}<Select.Item value={dc.url} label={dc.label}>{dc.label}</Select.Item>{/each}
              </Select.Content>
            </Select.Root>
          </div>
          <div class="space-y-1.5">
            <Label for="tuya-id">Access ID</Label>
            <Input id="tuya-id" placeholder="e.g. q4eusms3gyr7rsupqkjx" bind:value={accessId} />
          </div>
          <div class="space-y-1.5">
            <Label for="tuya-key">Access Secret</Label>
            <Input id="tuya-key" type="password" placeholder="from your project's Overview tab" bind:value={accessKey} />
          </div>
          <Button onclick={connect} disabled={connecting}>{connecting ? "Connecting…" : "Connect Tuya"}</Button>
          <p class="text-xs text-muted-foreground">
            Tuya's free <strong>Smart Home Basic Service</strong> can expire after a trial period — if devices stop
            responding later, renew it in your Tuya console.
          </p>
        </div>

      <!-- ===== Connected: device list + label editor ===== -->
      {:else}
        <div class="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {#if source === "tenant"}Connected via your Tuya account.{:else}Connected via managed credentials.{/if}
          </span>
          {#if source === "tenant"}
            <button type="button" class="interactive underline hover:text-foreground" onclick={disconnect}>Disconnect</button>
          {/if}
        </div>

        {#if devices.length === 0}
          <p class="rounded border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
            No devices yet. Make sure your account is linked in the Tuya console, then press <em>Refresh devices</em>.
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
      {/if}
    </Card.Content>
  </Card.Root>
</div>
