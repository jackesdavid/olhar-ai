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
  import { billing, boxes, type StorageStats, type Box } from "$lib/api";
  import { Input } from "$lib/components/ui/input";
  import { dict, lang, setLang, type Lang } from "$lib/i18n";

  const t = $derived(dict[lang()].settings);
  const c = $derived(dict[lang()].common);
  const langs: Lang[] = ["en", "pt"];

  let settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  let loading = $state(true);
  let saving = $state(false);
  let boxIds = $state<string[]>([]);
  // #120: a box that reported working GPU video decode (null = none capable → HD is CPU-only).
  let gpuBox = $state<Box | null>(null);
  function hwLabel(m: string): string {
    return { vaapi: "VAAPI", qsv: "QuickSync", cuda: "CUDA", videotoolbox: "VideoToolbox", v4l2m2m: "V4L2" }[m] ?? m;
  }
  const hdOn = () => settings.face_capture_stream === "hd";
  function setHd(on: boolean) { settings.face_capture_stream = on ? "hd" : "preview"; }
  let storageStats = $state<(StorageStats & { boxId: string })[]>([]);
  /** Does this tenant's plan actually sell cloud recording? Without it nothing is ever uploaded,
   *  so "delete the local copy once it's in the cloud" would delete the only copy there is. */
  let cloudRecording = $state(false);

  // Settings split into tabs — the page grew past one scroll of Cards.
  const TAB_IDS = ["general", "cameras", "motion"] as const;
  const TABS = $derived([
    { id: "general", label: t.tabGeneral },
    { id: "cameras", label: t.tabCameras },
    { id: "motion", label: t.tabMotion },
  ] as const);
  let tab = $state<(typeof TAB_IDS)[number]>("general");

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

  onMount(async () => {
    try {
      const [loaded, allBoxes] = await Promise.all([
        data.getSettings(),
        boxes.list(),
      ]);
      settings = { ...DEFAULT_SETTINGS, ...loaded };
      boxIds = allBoxes.map((b) => b.id);
      // #120: the GPU-decode method a box reported (for the HD face-capture gate). First capable box.
      gpuBox = allBoxes.find((b) => b.hwaccel && b.hwaccel !== "none") ?? null;
      // cameras_always_warm + pre_roll_secs live in each box's recording_config.
      const firstBox = allBoxes[0];
      if (firstBox?.recording_config != null) {
        settings.cameras_always_warm = firstBox.recording_config.cameras_always_warm ?? false;
        settings.face_capture_stream = firstBox.recording_config.face_capture_stream ?? "preview";
        settings.pre_roll_secs = firstBox.recording_config.pre_roll_secs ?? 10;
        settings.motion_var_threshold = firstBox.recording_config.motion_var_threshold ?? 16;
        settings.motion_fps = firstBox.recording_config.motion_fps ?? 4;
        settings.motion_floor_pct = firstBox.recording_config.motion_floor_pct ?? 0.5;
        settings.motion_min_interval_secs = firstBox.recording_config.motion_min_interval_secs ?? 4;
        settings.motion_min_area = firstBox.recording_config.motion_min_area ?? 300;
        settings.motion_proc_w = firstBox.recording_config.motion_proc_w ?? 480;
        settings.motion_proc_h = firstBox.recording_config.motion_proc_h ?? 270;
        settings.motion_history = firstBox.recording_config.motion_history ?? 333;
        settings.max_storage_mb = firstBox.recording_config.max_storage_mb ?? 10000;
        settings.recordings_path = firstBox.recording_config.recordings_path ?? "/opt/sentinel-edge/recordings";
        settings.delete_local_after_upload = firstBox.recording_config.delete_local_after_upload ?? false;
      }
      // Mirror control's own rule for offering the edge an upload ticket (`upload_ticket` →
      // `tenant_retention_days`): an active/trialing subscription on a plan that sells retention.
      // Best-effort — a billing hiccup must not block the settings page.
      loadCloudRecording();
      // Load storage stats for each box in the background (best-effort).
      for (const b of allBoxes) {
        boxes.storageStats(b.id).then((s) => {
          storageStats = [...storageStats.filter((x) => x.boxId !== b.id), { ...s, boxId: b.id }];
        }).catch(() => {});
      }
    } catch (err) {
      console.error("Failed to load settings", err);
      toast.error(t.loadFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  });

  async function loadCloudRecording() {
    try {
      const [sub, plans] = await Promise.all([billing.subscription(), billing.plans()]);
      const plan = plans.find((p) => p.id === sub?.plan_id);
      cloudRecording = (plan?.retention_days ?? 0) > 0;
    } catch {
      cloudRecording = false; // can't prove the plan sells it → don't offer to delete the only copy
    }
  }

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
            face_capture_stream: snap.face_capture_stream,
            motion_var_threshold: snap.motion_var_threshold,
            motion_fps: snap.motion_fps,
            motion_floor_pct: snap.motion_floor_pct,
            motion_min_interval_secs: snap.motion_min_interval_secs,
            motion_min_area: snap.motion_min_area,
            motion_proc_w: snap.motion_proc_w,
            motion_proc_h: snap.motion_proc_h,
            motion_history: snap.motion_history,
            // Never persist "delete the local copy" for a tenant with nowhere to copy it to.
            delete_local_after_upload: cloudRecording && snap.delete_local_after_upload,
          })
        )
      );
      toast.success(t.saved);
    } catch (err) {
      console.error("Failed to save settings", err);
      toast.error(t.saveFailed, { description: String(err) });
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

  function storageStepIndex(mb: number): number {
    const exact = STORAGE_STEPS.indexOf(mb);
    if (exact >= 0) return exact;
    return STORAGE_STEPS.findIndex((s) => s >= mb);
  }

  /** With eviction on, the box's disk is a spool for clips in transit — not an archive. */
  function storageLimitLabel(): string {
    return settings.delete_local_after_upload ? t.spoolSize : t.maxStorage;
  }

  function storageLimitHint(): string {
    return settings.delete_local_after_upload ? t.spoolHint : t.maxStorageHint;
  }

  function localRetentionHint(): string {
    return cloudRecording ? t.deleteLocalHint : t.deleteLocalNoPlan;
  }

  function sensitivityLabel(varThreshold: number): string {
    if (varThreshold <= 8) return t.sensVeryHigh;
    if (varThreshold <= 16) return t.sensHigh;
    if (varThreshold <= 32) return t.sensMedium;
    if (varThreshold <= 64) return t.sensLow;
    return t.sensVeryLow;
  }
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={SettingsIcon}
  >
    {#snippet actions()}
      <Button onclick={save} disabled={loading || saving}>
        {saving ? t.saving : c.save}
      </Button>
    {/snippet}
  </PageHeader>

  <div class="flex gap-1 border-b">
    {#each TABS as tb (tb.id)}
      <button
        type="button"
        class="-mb-px border-b-2 px-4 py-2 text-sm font-medium transition-colors {tab === tb.id
          ? 'border-primary text-foreground'
          : 'border-transparent text-muted-foreground hover:text-foreground'}"
        onclick={() => (tab = tb.id)}
      >
        {tb.label}
      </button>
    {/each}
  </div>

  {#if tab === "general"}
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
  {/if}

  {#if tab === "cameras"}
  <Card.Root>
    <Card.Header>
      <Card.Title>{t.camerasTitle}</Card.Title>
      <Card.Description>{t.camerasDescription}</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-6">
      <div class="flex items-center justify-between">
        <div class="space-y-0.5">
          <Label for="cameras-always-warm">{t.alwaysWarm}</Label>
          <p class="text-xs text-muted-foreground">{t.alwaysWarmHint}</p>
        </div>
        <Switch
          id="cameras-always-warm"
          bind:checked={settings.cameras_always_warm}
          disabled={loading}
        />
      </div>

      <Separator />

      <div class="flex items-center justify-between">
        <div class="space-y-0.5">
          <Label for="hd-face-capture">{t.hdFaceCapture}</Label>
          <p class="text-xs text-muted-foreground">
            {t.hdFaceCaptureHint}
            {#if gpuBox}
              <span class="text-foreground">{t.gpuDecodeYes(hwLabel(gpuBox.hwaccel ?? ""))}</span>
            {:else}
              <span>{t.gpuDecodeNo}</span>
            {/if}
            {t.takesEffectRestart}
          </p>
        </div>
        <Switch
          id="hd-face-capture"
          checked={hdOn()}
          onCheckedChange={setHd}
          disabled={loading}
        />
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{t.maxPreRoll}</Label>
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
        <p class="text-xs text-muted-foreground">{t.maxPreRollHint}</p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between gap-4">
          <div class="space-y-0.5">
            <Label for="delete-local-after-upload">{t.deleteLocal}</Label>
            <p class="text-xs text-muted-foreground">{localRetentionHint()}</p>
          </div>
          <Switch
            id="delete-local-after-upload"
            bind:checked={settings.delete_local_after_upload}
            disabled={loading || !cloudRecording}
          />
        </div>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{storageLimitLabel()}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {formatMb(settings.max_storage_mb)}
          </span>
        </div>
        <Slider
          type="single"
          min={0}
          max={STORAGE_STEPS.length - 1}
          step={1}
          value={storageStepIndex(settings.max_storage_mb)}
          onValueChange={(v) => { settings.max_storage_mb = STORAGE_STEPS[v] ?? STORAGE_STEPS[STORAGE_STEPS.length - 1]; }}
          disabled={loading}
        />
        <p class="text-xs text-muted-foreground">
          {storageLimitHint()}
          {t.fullDiskBefore}<strong>edge:full-disk</strong>{t.fullDiskAfter}
        </p>
      </div>

      <Separator />

      <div class="space-y-2">
        <Label>{t.recordingsFolder}</Label>
        <Input
          class="font-mono text-sm"
          placeholder="/opt/sentinel-edge/recordings"
          bind:value={settings.recordings_path}
          disabled={loading}
        />
        <p class="text-xs text-muted-foreground">{t.recordingsFolderHint}</p>
      </div>

      {#if storageStats.length > 0}
        <Separator />
        <div class="space-y-3">
          <Label class="text-xs text-muted-foreground">{t.currentUsage}</Label>
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
              <p class="text-xs text-muted-foreground">{t.clipsUsed(s.clip_count, s.pct)}</p>
            </div>
          {/each}
        </div>
      {/if}
    </Card.Content>
  </Card.Root>
  {/if}

  {#if tab === "motion"}
  <Card.Root>
    <Card.Header>
      <Card.Title>{t.motionTitle}</Card.Title>
      <Card.Description>{t.motionDescription}</Card.Description>
    </Card.Header>
    <Card.Content class="space-y-6">
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{t.sensitivity}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {sensitivityLabel(settings.motion_var_threshold)} ({settings.motion_var_threshold})
          </span>
        </div>
        <Slider type="single" min={4} max={128} step={4}
          bind:value={settings.motion_var_threshold} disabled={loading} />
        <p class="text-xs text-muted-foreground">{t.sensitivityHint}</p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{t.fps}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">{settings.motion_fps} fps</span>
        </div>
        <Slider type="single" min={1} max={15} step={1}
          bind:value={settings.motion_fps} disabled={loading} />
        <p class="text-xs text-muted-foreground">{t.fpsHint}</p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{t.motionFloor}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">{settings.motion_floor_pct}%</span>
        </div>
        <Slider type="single" min={0.1} max={5} step={0.1}
          bind:value={settings.motion_floor_pct} disabled={loading} />
        <p class="text-xs text-muted-foreground">{t.motionFloorHint}</p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{t.debounce}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">{settings.motion_min_interval_secs}s</span>
        </div>
        <Slider type="single" min={0} max={30} step={1}
          bind:value={settings.motion_min_interval_secs} disabled={loading} />
        <p class="text-xs text-muted-foreground">{t.debounceHint}</p>
      </div>

      <Separator />

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>{t.history}</Label>
          <span class="text-sm tabular-nums text-muted-foreground">
            {t.historyFrames(settings.motion_history, Math.round(settings.motion_history / Math.max(1, settings.motion_fps)))}
          </span>
        </div>
        <Slider type="single" min={30} max={1000} step={10}
          bind:value={settings.motion_history} disabled={loading} />
        <p class="text-xs text-muted-foreground">{t.historyHint}</p>
      </div>

      <Separator />

      <div class="grid grid-cols-3 gap-4">
        <div class="space-y-2">
          <Label>{t.minBlobArea}</Label>
          <Input type="number" min={0} bind:value={settings.motion_min_area} disabled={loading} />
          <p class="text-xs text-muted-foreground">{t.minBlobAreaHint}</p>
        </div>
        <div class="space-y-2">
          <Label>{t.procWidth}</Label>
          <Input type="number" min={1} bind:value={settings.motion_proc_w} disabled={loading} />
          <p class="text-xs text-muted-foreground">{t.procWidthHint}</p>
        </div>
        <div class="space-y-2">
          <Label>{t.procHeight}</Label>
          <Input type="number" min={1} bind:value={settings.motion_proc_h} disabled={loading} />
          <p class="text-xs text-muted-foreground">{t.procHeightHint}</p>
        </div>
      </div>
    </Card.Content>
  </Card.Root>
  {/if}
</div>
