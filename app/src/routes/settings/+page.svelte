<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { toast } from "svelte-sonner";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Slider } from "$lib/components/ui/slider";
  import { Separator } from "$lib/components/ui/separator";

  import { DEFAULT_SETTINGS, type Settings, type Transport } from "$lib/types";

  let settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  let loading = $state(true);
  let saving = $state(false);

  const transportOptions: { value: Transport; label: string }[] = [
    { value: "tcp", label: "TCP" },
    { value: "udp", label: "UDP" },
    { value: "auto", label: "Auto" },
  ];

  const transportLabel = $derived(
    transportOptions.find((o) => o.value === settings.transport)?.label ?? "Auto"
  );

  // bits-ui sliders use an array value; bridge them to the scalar fields.
  const gainFields = [
    { key: "master", label: "Master" },
    { key: "voice", label: "Voice" },
    { key: "ui", label: "UI" },
    { key: "ambient", label: "Ambient" },
  ] as const;

  onMount(async () => {
    try {
      const loaded = await invoke<Settings>("get_settings");
      settings = { ...DEFAULT_SETTINGS, ...loaded };
    } catch (err) {
      console.error("Failed to load settings", err);
      toast.error("Failed to load settings", {
        description: String(err),
      });
    } finally {
      loading = false;
    }
  });

  async function save() {
    saving = true;
    try {
      await invoke("set_settings", { settings: $state.snapshot(settings) });
      toast.success("Settings saved");
    } catch (err) {
      console.error("Failed to save settings", err);
      toast.error("Failed to save settings", {
        description: String(err),
      });
    } finally {
      saving = false;
    }
  }

  function pct(v: number): string {
    return `${Math.round(v * 100)}%`;
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Settings</h1>
      <p class="text-sm text-muted-foreground">
        Audio, voice, and connection preferences.
      </p>
    </div>
    <Button onclick={save} disabled={loading || saving}>
      {saving ? "Saving…" : "Save"}
    </Button>
  </div>

  <Card.Root>
    <Card.Header>
      <Card.Title>Audio</Card.Title>
      <Card.Description>Output gains, from muted to full.</Card.Description>
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
      <Card.Title>Voice & Interaction</Card.Title>
      <Card.Description>Speech synthesis and input behavior.</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-6">
      <div class="flex items-center justify-between">
        <div class="space-y-0.5">
          <Label for="voice-enabled">Voice enabled</Label>
          <p class="text-xs text-muted-foreground">
            Allow the console to speak responses aloud.
          </p>
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
          <Label>Typing speed</Label>
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
        <p class="text-xs text-muted-foreground">
          How fast streamed text is revealed (1–8).
        </p>
      </div>

      <Separator />

      <div class="flex items-center justify-between">
        <div class="space-y-0.5">
          <Label for="stt-autosend">Auto-send transcripts</Label>
          <p class="text-xs text-muted-foreground">
            Submit speech-to-text input automatically when you stop speaking.
          </p>
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
      <Card.Title>Connection</Card.Title>
      <Card.Description>Network transport for the backend link.</Card.Description>
    </Card.Header>
    <Card.Content>
      <div class="space-y-2">
        <Label>Transport</Label>
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
</div>
