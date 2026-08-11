<script lang="ts">
  // Settings → General: language, passkeys, audio, voice, connection, approval limits.
  //
  // Passkeys live entirely here rather than in the page: nothing outside this tab reads them, and
  // the page's job is loading and saving the settings blob, not owning every widget's state.
  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Slider } from "$lib/components/ui/slider";
  import { Separator } from "$lib/components/ui/separator";
  import { toast } from "svelte-sonner";
  import { onMount } from "svelte";

  import type { Settings, Transport } from "$lib/types";
  import { dict, lang, setLang, locale, type Lang } from "$lib/i18n";
  import {
    registerPasskey,
    listPasskeys,
    deletePasskey,
    passkeysSupported,
    type PasskeyMeta,
  } from "$lib/auth.svelte";

  let {
    settings,
    loading,
    gateMaxPending = $bindable(),
  }: {
    /** The `$state` blob owned by the page; mutations here reach it through the proxy. */
    settings: Settings;
    loading: boolean;
    /** Device Gate: most unanswered requests one phone may hold (#176). Tenant-scoped, so it is
     *  saved through `/settings` rather than with the UI-local blob. */
    gateMaxPending: number;
  } = $props();

  const t = $derived(dict[lang()].settings);
  const c = $derived(dict[lang()].common);
  const w = $derived(dict[lang()].webauthn);
  const langs: Lang[] = ["en", "pt"];

  // FIDO2 passkeys (card #147, §2 E0).
  let passkeys = $state<PasskeyMeta[]>([]);
  let pkBusy = $state(false);

  function pkName(pk: PasskeyMeta): string {
    return pk.nickname?.trim() || "Passkey";
  }
  function pkLastUsed(pk: PasskeyMeta): string {
    return pk.last_used_at ? new Date(pk.last_used_at).toLocaleDateString(locale()) : w.never;
  }
  async function loadPasskeys() {
    try {
      passkeys = await listPasskeys();
    } catch {
      /* leave empty */
    }
  }
  async function addPasskey() {
    if (pkBusy) return;
    pkBusy = true;
    try {
      await registerPasskey();
      toast.success(w.added);
      await loadPasskeys();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : w.failed);
    } finally {
      pkBusy = false;
    }
  }
  async function removePasskey(id: string) {
    try {
      await deletePasskey(id);
      toast.success(w.removed);
      await loadPasskeys();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : w.failed);
    }
  }

  onMount(loadPasskeys);

  const transportOptions: { value: Transport; label: string }[] = $derived([
    { value: "tcp", label: "TCP" },
    { value: "udp", label: "UDP" },
    { value: "auto", label: t.transportAuto },
  ]);

  const transportLabel = $derived(
    transportOptions.find((o) => o.value === settings.transport)?.label ?? t.transportAuto
  );

  // bits-ui sliders use an array value; bridge them to the scalar fields.
  const gainFields = $derived([
    { key: "master", label: t.gainMaster },
    { key: "voice", label: t.gainVoice },
    { key: "ui", label: t.gainUi },
    { key: "ambient", label: t.gainAmbient },
  ] as const);

  function pct(v: number): string {
    return `${Math.round(v * 100)}%`;
  }
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{c.language}</Card.Title>
  </Card.Header>
  <Card.Content>
    <div class="inline-flex items-center rounded-lg border p-0.5 text-sm font-semibold">
      {#each langs as l (l)}
        <button
          type="button"
          onclick={() => setLang(l)}
          class="rounded-md px-3 py-1.5 uppercase tracking-wide transition-colors"
          class:bg-primary={lang() === l}
          class:text-primary-foreground={lang() === l}
          class:text-muted-foreground={lang() !== l}
          class:hover:text-foreground={lang() !== l}
        >{l}</button>
      {/each}
    </div>
  </Card.Content>
</Card.Root>

{#if passkeysSupported()}
  <Card.Root>
    <Card.Header>
      <Card.Title>{w.title}</Card.Title>
      <Card.Description>{w.subtitle}</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-4">
      {#if passkeys.length === 0}
        <p class="text-sm text-muted-foreground">{w.none}</p>
      {:else}
        <ul class="space-y-2">
          {#each passkeys as pk (pk.id)}
            <li class="flex items-center justify-between rounded-lg border p-3">
              <div class="min-w-0">
                <div class="truncate text-sm font-medium">{pkName(pk)}</div>
                <div class="text-xs text-muted-foreground">{w.lastUsed}: {pkLastUsed(pk)}</div>
              </div>
              <Button variant="ghost" size="sm" onclick={() => removePasskey(pk.id)}>{w.remove}</Button>
            </li>
          {/each}
        </ul>
      {/if}
      <Button variant="outline" onclick={addPasskey} disabled={pkBusy}>{w.addKey}</Button>
    </Card.Content>
  </Card.Root>
{/if}

<Card.Root>
  <Card.Header>
    <Card.Title>{t.audio}</Card.Title>
    <Card.Description>{t.audioDescription}</Card.Description>
  </Card.Header>
  <Card.Content class="space-y-6">
    {#each gainFields as field (field.key)}
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{field.label}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {pct(settings[field.key])}
          </span>
        </div>
        <Slider
          type="single"
          min={0}
          max={1}
          step={0.01}
          bind:value={settings[field.key]}
          disabled={loading}
        />
      </div>
    {/each}
  </Card.Content>
</Card.Root>

<Card.Root>
  <Card.Header>
    <Card.Title>{t.voiceTitle}</Card.Title>
    <Card.Description>{t.voiceDescription}</Card.Description>
  </Card.Header>
  <Card.Content class="space-y-6">
    <div class="flex items-center justify-between">
      <div class="space-y-0.5">
        <Label for="voice-enabled">{t.voiceEnabled}</Label>
        <p class="text-xs text-muted-foreground">{t.voiceEnabledHint}</p>
      </div>
      <Switch
        id="voice-enabled"
        bind:checked={settings.voice_enabled}
        disabled={loading}
      />
    </div>

    <Separator />

    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <Label>{t.typingSpeed}</Label>
        <span class="text-sm tabular-nums text-muted-foreground">
          {settings.typing_speed}
        </span>
      </div>
      <Slider
        type="single"
        min={1}
        max={8}
        step={1}
        bind:value={settings.typing_speed}
        disabled={loading}
      />
      <p class="text-xs text-muted-foreground">{t.typingSpeedHint}</p>
    </div>

    <Separator />

    <div class="flex items-center justify-between">
      <div class="space-y-0.5">
        <Label for="stt-autosend">{t.autoSend}</Label>
        <p class="text-xs text-muted-foreground">{t.autoSendHint}</p>
      </div>
      <Switch
        id="stt-autosend"
        bind:checked={settings.stt_autosend}
        disabled={loading}
      />
    </div>
  </Card.Content>
</Card.Root>

<Card.Root>
  <Card.Header>
    <Card.Title>{t.approvals}</Card.Title>
    <Card.Description>{t.approvalsDescription}</Card.Description>
  </Card.Header>
  <Card.Content>
    <div class="space-y-2">
      <Label for="gate-max-pending">{t.approvalsCap}</Label>
      <Input
        id="gate-max-pending"
        class="w-32"
        type="number"
        min={1}
        max={100}
        bind:value={gateMaxPending}
        disabled={loading}
      />
      <p class="text-xs text-muted-foreground">{t.approvalsCapHint}</p>
    </div>
  </Card.Content>
</Card.Root>

<Card.Root>
  <Card.Header>
    <Card.Title>{t.connection}</Card.Title>
    <Card.Description>{t.connectionDescription}</Card.Description>
  </Card.Header>
  <Card.Content>
    <div class="space-y-2">
      <Label>{t.transport}</Label>
      <Select.Root type="single" bind:value={settings.transport} disabled={loading}>
        <Select.Trigger class="w-60">
          {transportLabel}
        </Select.Trigger>
        <Select.Content>
          {#each transportOptions as opt (opt.value)}
            <Select.Item value={opt.value} label={opt.label}>
              {opt.label}
            </Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </div>
  </Card.Content>
</Card.Root>
