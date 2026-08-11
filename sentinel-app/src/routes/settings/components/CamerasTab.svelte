<script lang="ts">
  // Settings → Cameras: live-feed and recording behaviour, plus what the boxes are actually using.
  //
  // Every value here is a per-box `recording_config` key except `face_capture_stream`, which the
  // tenant chooses once and control overlays onto each box's config (#174).
  import * as Card from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Slider } from "$lib/components/ui/slider";
  import { Separator } from "$lib/components/ui/separator";

  import type { Settings } from "$lib/types";
  import type { Box, StorageStats } from "$lib/api";
  import { dict, lang } from "$lib/i18n";

  let {
    settings,
    loading,
    gpuBox,
    cloudRecording,
    storageStats,
  }: {
    settings: Settings;
    loading: boolean;
    /** #120: a box that reported working GPU video decode (null = none capable → HD is CPU-only). */
    gpuBox: Box | null;
    /** Does the plan actually sell cloud recording? Without it "delete the local copy" would
     *  delete the only copy there is. */
    cloudRecording: boolean;
    storageStats: (StorageStats & { boxId: string })[];
  } = $props();

  const t = $derived(dict[lang()].settings);

  function hwLabel(m: string): string {
    return { vaapi: "VAAPI", qsv: "QuickSync", cuda: "CUDA", videotoolbox: "VideoToolbox", v4l2m2m: "V4L2" }[m] ?? m;
  }
  const hdOn = () => settings.face_capture_stream === "hd";
  function setHd(on: boolean) { settings.face_capture_stream = on ? "hd" : "preview"; }

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

  function usageColor(pct: number): string {
    if (pct >= 90) return "bg-destructive";
    if (pct >= 75) return "bg-amber-500";
    return "bg-primary";
  }

  function usageTextClass(pct: number): string {
    return pct >= 90 ? "text-destructive font-semibold" : "text-muted-foreground";
  }
</script>

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
          <div class="space-y-1.5">
            <div class="flex items-center justify-between text-xs">
              <span class="text-muted-foreground truncate font-mono">{s.path}</span>
              <span class="{usageTextClass(s.pct)} shrink-0 pl-2 tabular-nums">
                {formatBytes(s.used_bytes)} / {formatBytes(s.max_bytes)}
              </span>
            </div>
            <div class="h-2 w-full overflow-hidden rounded-full bg-muted">
              <div class="h-full rounded-full transition-all {usageColor(s.pct)}" style="width: {s.pct}%"></div>
            </div>
            <p class="text-xs text-muted-foreground">{t.clipsUsed(s.clip_count, s.pct)}</p>
          </div>
        {/each}
      </div>
    {/if}
  </Card.Content>
</Card.Root>
