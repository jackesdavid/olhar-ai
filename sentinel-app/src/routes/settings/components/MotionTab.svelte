<script lang="ts">
  // Settings → Detection: the MOG2 motion tuning the edge reads out of each box's recording_config.
  import * as Card from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Slider } from "$lib/components/ui/slider";
  import { Separator } from "$lib/components/ui/separator";

  import type { Settings } from "$lib/types";
  import { dict, lang } from "$lib/i18n";

  let { settings, loading }: { settings: Settings; loading: boolean } = $props();

  const t = $derived(dict[lang()].settings);

  function sensitivityLabel(varThreshold: number): string {
    if (varThreshold <= 8) return t.sensVeryHigh;
    if (varThreshold <= 16) return t.sensHigh;
    if (varThreshold <= 32) return t.sensMedium;
    if (varThreshold <= 64) return t.sensLow;
    return t.sensVeryLow;
  }

  /** The adaptation window the history slider actually buys, in seconds. */
  function historySecs(): number {
    return Math.round(settings.motion_history / Math.max(1, settings.motion_fps));
  }
</script>

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
          {t.historyFrames(settings.motion_history, historySecs())}
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
