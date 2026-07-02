<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import { onMount } from "svelte";
  import { data } from "$lib/data";
  import { toast } from "svelte-sonner";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Slider } from "$lib/components/ui/slider";
  import { Separator } from "$lib/components/ui/separator";

  import { DEFAULT_SETTINGS, type Settings, type Transport } from "$lib/types";
  import { boxes, type StorageStats } from "$lib/api";
  import { Input } from "$lib/components/ui/input";

  let settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  let loading = $state(true);
  let saving = $state(false);
  let boxIds = $state<string[]>([]);
  let storageStats = $state<(StorageStats & { boxId: string })[]>([]);

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
      const [loaded, allBoxes] = await Promise.all([
        data.getSettings(),
        boxes.list(),
      ]);
      settings = { ...DEFAULT_SETTINGS, ...loaded };
      boxIds = allBoxes.map((b) => b.id);
      // cameras_always_warm + pre_roll_secs live in each box's recording_config.
      const firstBox = allBoxes[0];
      if (firstBox?.recording_config != null) {
        settings.cameras_always_warm = firstBox.recording_config.cameras_always_warm ?? false;
        settings.pre_roll_secs = firstBox.recording_config.pre_roll_secs ?? 10;
        settings.motion_var_threshold = firstBox.recording_config.motion_var_threshold ?? 16;
        settings.max_storage_mb = firstBox.recording_config.max_storage_mb ?? 10000;
        settings.recordings_path = firstBox.recording_config.recordings_path ?? "/opt/sentinel-edge/recordings";
      }
      // Load storage stats for each box in the background (best-effort).
      for (const b of allBoxes) {
        boxes.storageStats(b.id).then((s) => {
          storageStats = [...storageStats.filter((x) => x.boxId !== b.id), { ...s, boxId: b.id }];
        }).catch(() => {});
      }
    } catch (err) {
      console.error("Failed to load settings", err);
      toast.error("Failed to load settings", { description: String(err) });
    } finally {
      loading = false;
    }
  });

  async function save() {
    saving = true;
    try {
      const snap = $state.snapshot(settings);
      await data.setSettings(snap);
      // cameras_always_warm + pre_roll_secs are edge behaviors — persist to every box's recording_config.
      await Promise.all(
        boxIds.map((id) =>
          boxes.patchRecordingConfig(id, {
            pre_roll_secs: snap.pre_roll_secs,
            max_storage_mb: snap.max_storage_mb,
            recordings_path: snap.recordings_path,
            codec: "copy",
            cameras_always_warm: snap.cameras_always_warm,
            motion_var_threshold: snap.motion_var_threshold,
          })
        )
      );
      toast.success("Settings saved");
    } catch (err) {
      console.error("Failed to save settings", err);
      toast.error("Failed to save settings", { description: String(err) });
    } finally {
      saving = false;
    }
  }

  function pct(v: number): string {
    return `${Math.round(v * 100)}%`;
  }

  function formatBytes(bytes: number): string {
    if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
    if (bytes >= 1e9)  return `${(bytes / 1e9).toFixed(1)} GB`;
    if (bytes >= 1e6)  return `${(bytes / 1e6).toFixed(1)} MB`;
    if (bytes >= 1e3)  return `${(bytes / 1e3).toFixed(0)} KB`;
    return `${bytes} B`;
  }

  function formatMb(mb: number): string {
    if (mb >= 1024 * 1024) return `${(mb / (1024 * 1024)).toFixed(0)} TB`;
    if (mb >= 1024) return `${(mb / 1024).toFixed(0)} GB`;
    return `${mb} MB`;
  }

  const STORAGE_STEPS = [500, 1000, 2000, 5000, 10000, 20000, 50000, 100000];

  function sensitivityLabel(varThreshold: number): string {
    if (varThreshold <= 8) return "very high";
    if (varThreshold <= 16) return "high";
    if (varThreshold <= 32) return "medium";
    if (varThreshold <= 64) return "low";
    return "very low";
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Settings"
    description="Audio, voice, and connection preferences."
    icon={SettingsIcon}
  >
    {#snippet actions()}
      <Button onclick={save} disabled={loading || saving}>
        {saving ? "Saving…" : "Save"}
      </Button>
    {/snippet}
  </PageHeader>

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

  <Card.Root>
    <Card.Header>
      <Card.Title>Cameras</Card.Title>
      <Card.Description>Live feed and recording behaviour.</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-6">
      <div class="flex items-center justify-between">
        <div class="space-y-0.5">
          <Label for="cameras-always-warm">Keep camera feeds always on</Label>
          <p class="text-xs text-muted-foreground">
            Maintains a rolling pre-roll buffer per camera so recordings start with
            footage from before the trigger. Uses more bandwidth when nothing is watching.
          </p>
        </div>
        <Switch
          id="cameras-always-warm"
          bind:checked={settings.cameras_always_warm}
          disabled={loading}
        />
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>Max pre-roll</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {settings.pre_roll_secs}s
          </span>
        </div>
        <Slider
          type="single"
          min={0}
          max={60}
          step={5}
          bind:value={settings.pre_roll_secs}
          disabled={loading || !settings.cameras_always_warm}
        />
        <p class="text-xs text-muted-foreground">
          How many seconds of footage the ring buffer keeps before a recording trigger
          (0–60 s). The recording node can request up to this amount.
        </p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>Motion sensitivity</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {sensitivityLabel(settings.motion_var_threshold)}
          </span>
        </div>
        <Slider
          type="single"
          min={4}
          max={128}
          step={4}
          bind:value={settings.motion_var_threshold}
          disabled={loading}
        />
        <p class="text-xs text-muted-foreground">
          Controls how much pixel change triggers a motion event. Lower = more sensitive
          (reacts to subtle movement); higher = less sensitive (ignores small changes).
          Takes effect after the edge restarts.
        </p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>Max recording storage</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {formatMb(settings.max_storage_mb)}
          </span>
        </div>
        <Slider
          type="single"
          min={0}
          max={STORAGE_STEPS.length - 1}
          step={1}
          value={STORAGE_STEPS.indexOf(settings.max_storage_mb) >= 0
            ? STORAGE_STEPS.indexOf(settings.max_storage_mb)
            : STORAGE_STEPS.findIndex((s) => s >= settings.max_storage_mb)}
          onValueChange={(v) => { settings.max_storage_mb = STORAGE_STEPS[v] ?? STORAGE_STEPS[STORAGE_STEPS.length - 1]; }}
          disabled={loading}
        />
        <p class="text-xs text-muted-foreground">
          Old clips are auto-deleted (oldest first) when total recording size exceeds this limit.
          A workflow event <strong>edge:full-disk</strong> fires when usage reaches 90%.
        </p>
      </div>

      <Separator />

      <div class="space-y-2">
        <Label>Recordings folder</Label>
        <Input
          class="font-mono text-sm"
          placeholder="/opt/sentinel-edge/recordings"
          bind:value={settings.recordings_path}
          disabled={loading}
        />
        <p class="text-xs text-muted-foreground">
          Directory on the edge appliance where .mp4 clips are stored. Takes effect after the edge restarts.
        </p>
      </div>

      {#if storageStats.length > 0}
        <Separator />
        <div class="space-y-3">
          <Label class="text-xs text-muted-foreground">Current storage usage</Label>
          {#each storageStats as s (s.boxId)}
            {@const color = s.pct >= 90 ? "bg-destructive" : s.pct >= 75 ? "bg-amber-500" : "bg-primary"}
            <div class="space-y-1.5">
              <div class="flex items-center justify-between text-xs">
                <span class="text-muted-foreground truncate font-mono">{s.path}</span>
                <span class="{s.pct >= 90 ? 'text-destructive font-semibold' : 'text-muted-foreground'} shrink-0 pl-2 tabular-nums">
                  {formatBytes(s.used_bytes)} / {formatBytes(s.max_bytes)}
                </span>
              </div>
              <div class="h-2 w-full overflow-hidden rounded-full bg-muted">
                <div class="h-full rounded-full transition-all {color}" style="width: {s.pct}%"></div>
              </div>
              <p class="text-xs text-muted-foreground">{s.clip_count} clip{s.clip_count !== 1 ? "s" : ""} · {s.pct}% used</p>
            </div>
          {/each}
        </div>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
