<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Plug from "@lucide/svelte/icons/plug";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
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
  import type { TuyaDevice, TuyaDiagnosis } from "$lib/types";

  const t = $derived(dict[lang()].integrations);

  type Source = "tenant" | "operator" | "none";
  let source = $state<Source | null>(null);
  let devices = $state<TuyaDevice[]>([]);
  let loading = $state(true);
  let syncing = $state(false);
  // Where to send someone who wants a SECOND Smart Life account on this project, and whether that
  // link is the exact tab or just the project list (see `TuyaProvider`).
  let consoleUrl = $state<string | null>(null);
  let consoleExact = $state(false);
  let consolePaste = $state("");
  let savingConsole = $state(false);
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
  // Why the connection is not working (#182). Null = not checked yet.
  let diagnosis = $state<TuyaDiagnosis | null>(null);
  let checking = $state(false);
  let testing = $state<string | null>(null);
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
      const prov = await data
        .getTuyaProvider()
        .catch(() => ({ configured: false, source: "none" as Source, console_url: null, console_url_exact: false }));
      source = prov.source;
      consoleUrl = prov.console_url;
      consoleExact = prov.console_url_exact;
      devices = prov.configured ? await data.listTuyaDevices().catch(() => []) : [];
    } catch (err) {
      console.error("Failed to load Tuya integration", err);
      toast.error(t.loadFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  }

  /** Remember which console project these credentials belong to, so the link lands on the tab
   *  rather than the project list. Any tab's URL will do — we keep the identity and rebuild the
   *  tab ourselves. */
  async function saveConsoleUrl() {
    const url = consolePaste.trim();
    if (!url) return;
    savingConsole = true;
    try {
      await data.setTuyaConsoleUrl(url);
      consolePaste = "";
      await reload();
      toast.success(t.consoleLinkSaved);
    } catch (err) {
      toast.error(t.consoleLinkFailed, { description: String(err) });
    } finally {
      savingConsole = false;
    }
  }

  /** The localised sentence for a diagnosis kind. Keyed on the backend's stable `kind` rather
   *  than on a message it sent, so the copy lives here where it can be translated. */
  function diagMessage(d: TuyaDiagnosis): string {
    switch (d.kind) {
      case "signature": return t.diagSignature;
      case "not_subscribed": return t.diagNotSubscribed;
      case "wrong_data_center": return t.diagWrongDataCenter(d.suggested_region ?? "");
      case "no_linked_account": return t.diagNoLinkedAccount;
      case "no_devices": return t.diagNoDevices;
      case "network": return t.diagNetwork;
      case "ok": return t.diagOk(d.devices);
      default: return t.diagApi;
    }
  }
  /** Which checklist rows passed. A step only counts as reached once the ones before it did. */
  const DIAG_ORDER = ["credentials", "apis", "account", "devices"] as const;
  function stepState(d: TuyaDiagnosis, step: (typeof DIAG_ORDER)[number]): "ok" | "fail" | "idle" {
    if (d.ok) return "ok";
    const failedAt = DIAG_ORDER.indexOf(d.step as (typeof DIAG_ORDER)[number]);
    const here = DIAG_ORDER.indexOf(step);
    if (failedAt < 0) return "idle";
    if (here < failedAt) return "ok";
    if (here === failedAt) return "fail";
    return "idle";
  }
  function stepLabel(step: (typeof DIAG_ORDER)[number]): string {
    if (step === "credentials") return t.diagStepCredentials;
    if (step === "apis") return t.diagStepApis;
    if (step === "account") return t.diagStepAccount;
    return t.diagStepDevices;
  }

  /** Probe the credentials in the form (or the saved ones) and say what is wrong. Runs BEFORE
   *  saving, so a wrong data centre is caught without persisting it. */
  async function check(withForm: boolean) {
    checking = true;
    try {
      diagnosis = await data.diagnoseTuya(
        withForm
          ? { access_id: accessId.trim(), access_key: accessKey.trim(), endpoint }
          : undefined,
      );
    } catch (err) {
      toast.error(t.connectFailed, { description: String(err) });
    } finally {
      checking = false;
    }
  }

  /** Adopt the data centre the probe found. Deliberately a CLICK, not automatic: silently
   *  rewriting a saved endpoint is the kind of helpfulness nobody can debug later. */
  function useSuggestedRegion() {
    if (!diagnosis?.suggested_endpoint) return;
    endpoint = diagnosis.suggested_endpoint;
    diagnosis = null;
  }

  /** Prove the device answers. Reads status — changes nothing — and reports the round-trip. */
  async function testDevice(d: TuyaDevice) {
    testing = d.device_id;
    try {
      const r = await data.testTuyaDevice(d.device_id);
      toast.success(t.testOk(r.ms), {
        description: Object.entries(r.result.status ?? {})
          .map(([k, v]) => `${k}: ${v}`)
          .join(" · "),
      });
    } catch (err) {
      toast.error(t.testFailed, { description: String(err) });
    } finally {
      testing = null;
    }
  }

  async function connect() {
    if (!accessId.trim() || !accessKey.trim()) {
      toast.error(t.enterCreds);
      return;
    }
    connecting = true;
    try {
      // Diagnose BEFORE saving. `set_config` validates too, but it can only answer yes/no — this
      // is what turns "couldn't connect" into "your project is in Western America".
      const d = await data.diagnoseTuya({
        access_id: accessId.trim(),
        access_key: accessKey.trim(),
        endpoint,
      });
      if (d.step === "credentials" || d.step === "apis") {
        diagnosis = d;
        return; // nothing saved — the form still holds what they typed
      }
      diagnosis = d; // linked-account / no-devices are shown but do NOT block saving
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
      {#if consoleUrl && source === "tenant"}
        <!-- Linking another Smart Life account happens in the tenant's OWN Tuya console: there is
             no public API for the QR, so the product's whole contribution is knowing where the tab
             is. After the first account that tab shows a LIST, and the QR only returns behind an
             "Add App Account" button nobody finds by accident. -->
        <div class="rounded-lg border border-border bg-well p-3 text-sm">
          <p class="font-medium">{t.linkAnotherAccount}</p>
          <p class="mt-1 text-xs text-muted-foreground">{t.linkAnotherAccountHint}</p>
          <a
            href={consoleUrl}
            target="_blank"
            rel="noopener noreferrer"
            class="interactive mt-2 inline-flex items-center gap-1.5 text-xs font-medium underline"
          >
            {consoleExact ? t.openConsoleTab : t.openConsoleList}
            <ExternalLink class="size-3.5" />
          </a>
          {#if !consoleExact}
            <div class="mt-3 space-y-1">
              <p class="text-xs text-muted-foreground">{t.pasteConsoleUrl}</p>
              <div class="flex items-center gap-2">
                <Input
                  bind:value={consolePaste}
                  placeholder="https://us.platform.tuya.com/cloud/basic?id=p17…"
                  class="h-8"
                  autocomplete="off"
                  spellcheck="false"
                />
                <Button size="sm" onclick={saveConsoleUrl} disabled={savingConsole || !consolePaste.trim()}>
                  {t.saveConsole}
                </Button>
              </div>
            </div>
          {/if}
        </div>
      {/if}

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
          <div class="flex items-center gap-2">
            <Button onclick={connect} disabled={connecting}>{connecting ? t.connecting : t.connectTuya}</Button>
            <!-- Check WITHOUT saving. Someone who has not finished the console steps can find out
                 which one they are missing instead of guessing from a failed save. -->
            <Button variant="outline" onclick={() => check(true)} disabled={checking || !accessId.trim() || !accessKey.trim()}>
              {checking ? t.checking : t.checkConnection}
            </Button>
          </div>
          {@render diagPanel()}
          <p class="text-xs text-muted-foreground">{t.trialWarning}</p>
        </div>

      <!-- ===== Connected: device list + label editor ===== -->
      {:else}
        <div class="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {source === "tenant" ? t.viaYourAccount : t.viaManaged}
          </span>
          <div class="flex items-center gap-3">
            <button type="button" class="interactive underline hover:text-foreground" onclick={() => check(false)}>
              {checking ? t.checking : t.checkConnection}
            </button>
            {#if source === "tenant"}
              <button type="button" class="interactive underline hover:text-foreground" onclick={disconnect}>{t.disconnect}</button>
            {/if}
          </div>
        </div>
        {@render diagPanel()}

        {#if devices.length === 0}
          <p class="rounded border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
            {t.noDevices}
          </p>
        {:else}
          <ul class="divide-y divide-border">
            {#each devices as d (d.device_id)}
              {@const fns = functionsOf(d)}
              <li class="py-2.5">
                <div class="flex w-full items-center gap-2">
                  <button
                    type="button"
                    class="interactive flex min-w-0 flex-1 items-center justify-between gap-3 text-left"
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
                  <!-- Reads the device's status and changes nothing — the point is to SEE it
                       answer, and the round-trip time, before building a workflow on it. A
                       toggle would need a confirm: this may be a lock. -->
                  <Button
                    variant="ghost"
                    size="sm"
                    class="shrink-0"
                    disabled={testing === d.device_id}
                    onclick={() => testDevice(d)}
                  >
                    {testing === d.device_id ? t.testing : t.testDevice}
                  </Button>
                </div>

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

<!-- The four-step connection checklist (#182). Three of the failures it separates end in an empty
     device list, which is why "connected: yes/no" was never enough to act on. -->
{#snippet diagPanel()}
  {#if diagnosis}
    {@const d = diagnosis}
    <div class="space-y-2 rounded-lg border {d.ok ? 'border-border' : 'border-destructive/40'} bg-card p-3">
      <p class="text-xs font-medium text-muted-foreground">{t.diagTitle}</p>
      <ul class="space-y-1">
        {#each DIAG_ORDER as step (step)}
          {@const st = stepState(d, step)}
          <li class="flex items-center gap-2 text-sm">
            {#if st === "ok"}
              <Check class="size-4 shrink-0 text-primary" />
            {:else if st === "fail"}
              <X class="size-4 shrink-0 text-destructive" />
            {:else}
              <span class="size-4 shrink-0"></span>
            {/if}
            <span class={st === "idle" ? "text-muted-foreground" : ""}>{stepLabel(step)}</span>
          </li>
        {/each}
      </ul>
      <p class="text-sm {d.ok ? 'text-muted-foreground' : ''}">{diagMessage(d)}</p>
      {#if d.suggested_endpoint && d.suggested_region}
        <Button size="sm" onclick={useSuggestedRegion}>{t.diagUseRegion(d.suggested_region)}</Button>
      {/if}
      {#if d.detail}
        <!-- Tuya's own wording, kept as supporting detail rather than instead of ours: inventing a
             friendly message for a code we have not seen would be worse than showing the raw one. -->
        <p class="text-xs text-muted-foreground">{t.diagDetail}: {d.detail}{d.code ? ` (${d.code})` : ""}</p>
      {/if}
    </div>
  {/if}
{/snippet}
