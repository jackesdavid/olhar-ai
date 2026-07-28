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
  import { dict, lang } from "$lib/i18n";
  import type { TuyaDevice } from "$lib/types";

  const t = $derived(dict[lang()].integrations);

  type Source = "tenant" | "operator" | "none";
  let source = $state<Source | null>(null);
  let devices = $state<TuyaDevice[]>([]);
  let loading = $state(true);
  let syncing = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let edits = $state<Record<string, Record<string, string>>>({});
  let savingLabels = $state<string | null>(null);

  // Connect-your-account form.
  const DATA_CENTERS = $derived([
    { label: t.dcUs, url: "https://openapi.tuyaus.com" },
    { label: t.dcEu, url: "https://openapi.tuyaeu.com" },
    { label: t.dcCn, url: "https://openapi.tuyacn.com" },
    { label: t.dcIn, url: "https://openapi.tuyain.com" },
  ]);
  let accessId = $state("");
  let accessKey = $state("");
  let endpoint = $state("https://openapi.tuyaus.com");
  let connecting = $state(false);
  let showGuide = $state(true);
  const dcLabel = $derived(DATA_CENTERS.find((d) => d.url === endpoint)?.label ?? t.dataCenter);

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
      toast.error(t.loadFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  }

  async function connect() {
    if (!accessId.trim() || !accessKey.trim()) {
      toast.error(t.enterCreds);
      return;
    }
    connecting = true;
    try {
      await data.setTuyaConfig(accessId.trim(), accessKey.trim(), endpoint);
      toast.success(t.connectedToast);
      accessKey = ""; // don't keep the secret around in the field
      await reload();
      await sync();
    } catch (err) {
      console.error("Tuya connect failed", err);
      toast.error(t.connectFailed, { description: String(err) });
    } finally {
      connecting = false;
    }
  }

  async function disconnect() {
    try {
      await data.deleteTuyaConfig();
      toast.success(t.disconnected);
      devices = [];
      await reload();
    } catch (err) {
      toast.error(t.disconnectFailed, { description: String(err) });
    }
  }

  async function sync() {
    syncing = true;
    try {
      const { synced } = await data.syncTuyaDevices();
      toast.success(t.synced(synced));
      await reload();
    } catch (err) {
      console.error("Tuya sync failed", err);
      toast.error(t.syncFailed, { description: String(err) });
    } finally {
      syncing = false;
    }
  }

  async function saveLabels(d: TuyaDevice) {
    savingLabels = d.device_id;
    try {
      await data.setTuyaFunctionLabels(d.device_id, edits[d.device_id] ?? {});
      toast.success(t.namesSaved);
      await reload();
    } catch (err) {
      toast.error(t.namesFailed, { description: String(err) });
    } finally {
      savingLabels = null;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={Plug}
  />

  <Card.Root>
    <Card.Header>
      <div class="flex items-start justify-between gap-4">
        <div class="space-y-1">
          <Card.Title class="flex items-center gap-2">
            {t.tuya}
            {#if source === "tenant"}
              <Badge variant="secondary">{t.connected}</Badge>
            {:else if source === "operator"}
              <Badge variant="secondary">{t.connectedManaged}</Badge>
            {:else if source === "none"}
              <Badge variant="outline">{t.notConnected}</Badge>
            {/if}
          </Card.Title>
          <Card.Description>
            {t.tuyaDescriptionBefore}<strong>{t.controlNode}</strong>{t.tuyaDescriptionMiddle}<strong
              >{t.readNode}</strong
            >{t.tuyaDescriptionAfter}
          </Card.Description>
        </div>
        {#if source && source !== "none"}
          <Button variant="outline" onclick={sync} disabled={syncing || loading}>
            <RefreshCw class="mr-2 size-4 {syncing ? 'animate-spin' : ''}" />
            {syncing ? t.syncing : t.refreshDevices}
          </Button>
        {/if}
      </div>
    </Card.Header>
    <Card.Content class="space-y-4">
      {#if loading || source === null}
        <p class="text-sm text-muted-foreground">{t.loading}</p>

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
            {t.guideToggle}
          </button>
          {#if showGuide}
            <ol class="list-decimal space-y-2 border-t border-border px-6 py-3 text-sm text-muted-foreground">
              <li>
                {t.step1Before}<a
                  href="https://platform.tuya.com"
                  target="_blank"
                  rel="noopener"
                  class="inline-flex items-center gap-0.5 font-medium text-foreground underline"
                  >platform.tuya.com <ExternalLink class="size-3" /></a
                >{t.step1After}
              </li>
              <li>{t.step2}</li>
              <li>{t.step3}</li>
              <li>{t.step4}</li>
              <li>{t.step5}</li>
            </ol>
          {/if}
        </div>

        <div class="space-y-3">
          <div class="space-y-1.5">
            <Label for="tuya-dc">{t.dataCenter}</Label>
            <Select.Root type="single" bind:value={endpoint}>
              <Select.Trigger id="tuya-dc" class="w-full">{dcLabel}</Select.Trigger>
              <Select.Content>
                {#each DATA_CENTERS as dc (dc.url)}<Select.Item value={dc.url} label={dc.label}>{dc.label}</Select.Item>{/each}
              </Select.Content>
            </Select.Root>
          </div>
          <div class="space-y-1.5">
            <Label for="tuya-id">{t.accessId}</Label>
            <Input id="tuya-id" placeholder={t.accessIdPlaceholder} autocomplete="off" bind:value={accessId} />
          </div>
          <div class="space-y-1.5">
            <Label for="tuya-key">{t.accessSecret}</Label>
            <Input id="tuya-key" type="password" placeholder={t.accessSecretPlaceholder} autocomplete="off" bind:value={accessKey} />
          </div>
          <Button onclick={connect} disabled={connecting}>{connecting ? t.connecting : t.connectTuya}</Button>
          <p class="text-xs text-muted-foreground">{t.trialWarning}</p>
        </div>

      <!-- ===== Connected: device list + label editor ===== -->
      {:else}
        <div class="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {source === "tenant" ? t.viaYourAccount : t.viaManaged}
          </span>
          {#if source === "tenant"}
            <button type="button" class="interactive underline hover:text-foreground" onclick={disconnect}>{t.disconnect}</button>
          {/if}
        </div>

        {#if devices.length === 0}
          <p class="rounded border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
            {t.noDevices}
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
                        {d.category ?? t.deviceKind} · {t.controlCount(fns.length)}
                      </p>
                    </div>
                  </div>
                  <Badge variant={d.online ? "secondary" : "outline"} class="shrink-0">
                    {d.online ? t.online : t.offline}
                  </Badge>
                </button>

                {#if expanded[d.device_id]}
                  <div class="mt-3 space-y-2 pl-6">
                    {#if fns.length === 0}
                      <p class="text-xs text-muted-foreground">{t.noControls}</p>
                    {:else}
                      {#each fns as f (f.code)}
                        <div class="flex items-center gap-3">
                          <div class="w-40 shrink-0">
                            <p class="truncate font-mono text-xs">{f.code}</p>
                            <p class="text-[10px] text-muted-foreground">{f.type}</p>
                          </div>
                          <Input
                            class="h-8"
                            placeholder={t.friendlyNamePlaceholder}
                            value={edits[d.device_id]?.[f.code] ?? ""}
                            oninput={(e) => editLabel(d.device_id, f.code, e.currentTarget.value)}
                          />
                        </div>
                      {/each}
                      <div class="flex justify-end pt-1">
                        <Button size="sm" onclick={() => saveLabels(d)} disabled={savingLabels === d.device_id}>
                          {savingLabels === d.device_id ? t.savingNames : t.saveNames}
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
