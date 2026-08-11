<script lang="ts">
  // Settings (/settings). The page owns loading and saving; each tab owns its own widgets and the
  // helpers only it uses — see `./components/*Tab.svelte`.
  //
  // Three stores meet here, and they are NOT the same thing:
  //   * UI-local preferences (audio, voice, transport)     → `data.getSettings` / localStorage
  //   * per-box edge behaviour (recording, motion)         → `boxes.patchRecordingConfig`
  //   * workspace-wide product choices (the gate cap)      → `tenantSettings` (`/settings`)
  // `Settings` is the flat blob the widgets bind to; `save()` is where it fans back out.
  import PageHeader from "$lib/components/page-header.svelte";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import { onMount } from "svelte";
  import { data } from "$lib/data";
  import { toast } from "svelte-sonner";

  import { Button } from "$lib/components/ui/button";

  import GeneralTab from "./components/GeneralTab.svelte";
  import CamerasTab from "./components/CamerasTab.svelte";
  import MotionTab from "./components/MotionTab.svelte";

  import { DEFAULT_SETTINGS, type Settings } from "$lib/types";
  import { billing, boxes, tenantSettings, type StorageStats, type Box } from "$lib/api";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].settings);
  const c = $derived(dict[lang()].common);

  let settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  let loading = $state(true);
  let saving = $state(false);
  let boxIds = $state<string[]>([]);
  // #120: a box that reported working GPU video decode (null = none capable → HD is CPU-only).
  let gpuBox = $state<Box | null>(null);
  let storageStats = $state<(StorageStats & { boxId: string })[]>([]);
  /** Does this tenant's plan actually sell cloud recording? Without it nothing is ever uploaded,
   *  so "delete the local copy once it's in the cloud" would delete the only copy there is. */
  let cloudRecording = $state(false);
  /** #176: most unanswered Device Gate requests one phone may hold. Tenant-scoped, and the server
   *  decides what unset means — 10 here is only what the field shows before a choice is made. */
  let gateMaxPending = $state(10);

  // Settings split into tabs — the page grew past one scroll of Cards.
  const TAB_IDS = ["general", "cameras", "motion"] as const;
  const TABS = $derived([
    { id: "general", label: t.tabGeneral },
    { id: "cameras", label: t.tabCameras },
    { id: "motion", label: t.tabMotion },
  ] as const);
  let tab = $state<(typeof TAB_IDS)[number]>("general");

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
      loadTenantSettings();
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

  async function loadTenantSettings() {
    try {
      const ts = await tenantSettings.get();
      gateMaxPending = ts.device_gate_max_pending ?? gateMaxPending;
    } catch {
      /* keep the shown default; the server still decides what unset means */
    }
  }

  async function save() {
    saving = true;
    try {
      const snap = $state.snapshot(settings);
      await data.setSettings(snap);
      // Clamped rather than sent as typed: the number input accepts anything a keyboard can produce,
      // and control rejects out-of-range — which would fail the whole save over one stray digit.
      const cap = Math.min(100, Math.max(1, Math.round(gateMaxPending || 1)));
      gateMaxPending = cap;
      await tenantSettings.put({ device_gate_max_pending: cap });
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
    <GeneralTab {settings} {loading} bind:gateMaxPending />
  {/if}

  {#if tab === "cameras"}
    <CamerasTab {settings} {loading} {gpuBox} {cloudRecording} {storageStats} />
  {/if}

  {#if tab === "motion"}
    <MotionTab {settings} {loading} />
  {/if}
</div>
