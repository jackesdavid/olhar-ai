<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Cpu from "@lucide/svelte/icons/cpu";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { data, onEvent, type EdgeDiscovered, type ProbeResult, type ProbeStatus, type StreamingCamera } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Radar from "@lucide/svelte/icons/radar";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import ScrollText from "@lucide/svelte/icons/scroll-text";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import CameraStatus from "$lib/components/camera-status.svelte";
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import Check from "@lucide/svelte/icons/check";
  import XCircle from "@lucide/svelte/icons/circle-x";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import { dict, lang } from "$lib/i18n";
  import type { Device } from "$lib/types";

  const t = $derived(dict[lang()].devices);
  const c = $derived(dict[lang()].common);

  let devices = $state<Device[]>([]);
  let loading = $state(true);

  // Cameras the tenant's edge found on its LAN and pushed up.
  let edgeFound = $state<EdgeDiscovered[]>([]);
  // IPs whose credentials are set (the cloud never stores the creds, only this flag).
  let configuredIps = $state<Set<string>>(new Set());
  // A camera is "configured" once its credentials have been relayed to the edge.
  function isConfigured(d: Device): boolean {
    return configuredIps.has(d.ip);
  }

  // IPs the edge currently sees on the LAN (its latest ONVIF discovery sweep, pushed up).
  const discoveredIps = $derived(new Set(edgeFound.map((c) => c.ip)));
  // IPs the edge has a RESOLVED stream for (creds valid + reachable) — the precise signal.
  let streamingIps = $state<Set<string>>(new Set());
  // Codec the edge probed off each streaming camera's own SDP ("H264"/"H265"), keyed by IP —
  // `undefined` until reported (older edge build, or not probed yet).
  let codecByIp = $state<Map<string, string>>(new Map());

  // The status dot, using what the CLOUD actually knows (it never holds the RTSP URL — that's
  // edge-local — so the old rtsp_url check was always gray):
  //   online (green) = the edge resolved a working stream (valid creds + camera reachable)
  //   warn   (amber) = creds set but no stream yet (wrong creds / camera down / still resolving),
  //                    or the camera is visible on the LAN but has no credentials yet
  //   offline (gray) = not set up and not visible
  function cameraState(d: Device): "online" | "warn" | "offline" {
    // Prefer what the edge actually reported (#155). The inference below is a fallback for a box
    // that hasn't reported yet — it can only guess "is it streaming", never WHY it isn't.
    if (d.probe_status === "streaming") return "online";
    if (d.probe_status === "ok") return "warn";
    if (d.probe_status) return "offline";
    if (streamingIps.has(d.ip)) return "online";
    if (isConfigured(d) || discoveredIps.has(d.ip)) return "warn";
    return "offline";
  }
  function cameraTitle(d: Device): string {
    if (streamingIps.has(d.ip)) return t.stateOnline;
    const onLan = discoveredIps.has(d.ip);
    const configured = isConfigured(d);
    if (configured && onLan) return t.stateConfiguredOnLan;
    if (configured) return t.stateConfiguredOffline;
    if (onLan) return t.stateOnLan;
    return t.stateUnknown;
  }

  // ---- Detail dialog (edit name + credentials, test, snapshot) -------------
  let detailOpen = $state(false);
  let detail = $state<Device | null>(null);
  let editName = $state("");
  let editUser = $state("");
  let editPass = $state("");
  let editStreamMode = $state("split"); // #120 — "split" | "unified"
  let savingDetail = $state(false);
  // Credential test (card #88): the camera proves the creds before we save them.
  type TestState = "idle" | "testing" | "verified" | "failed";
  let testState = $state<TestState>("idle");
  let testSnapshotUrl = $state<string | null>(null);
  let verdict = $state<ProbeResult | null>(null);

  // ---- Add camera manually (card #97) -------------------------------------
  // For when discovery can't reach the camera — most often because the network drops the
  // WS-Discovery multicast, even though the camera itself is perfectly reachable. The user supplies
  // what discovery normally fills in (IP + ONVIF endpoint); the edge resolves the stream identically.
  //
  // Two source modes (#175). ONVIF is the normal path: the edge asks the camera where its stream
  // is. "rtsp" is for a source that has no ONVIF at all — a phone running an RTSP app is the case
  // that motivated it — where the user supplies the stream URL and there is nothing to resolve.
  type SourceMode = "onvif" | "rtsp";
  let manualOpen = $state(false);
  let manualMode = $state<SourceMode>("onvif");
  let manualRtspUrl = $state("");
  let manualName = $state("");
  let manualIp = $state("");
  let manualPort = $state("2020");
  let manualUser = $state("");
  let manualPass = $state("");
  let manualState = $state<TestState>("idle");
  let manualVerdict = $state<ProbeResult | null>(null);
  let manualSnapshotUrl = $state<string | null>(null);
  let savingManual = $state(false);

  function openManual() {
    manualMode = "onvif";
    manualRtspUrl = "";
    manualName = "";
    manualIp = "";
    manualPort = "2020";
    manualUser = "";
    manualPass = "";
    manualState = "idle";
    manualVerdict = null;
    manualSnapshotUrl = null;
    manualOpen = true;
  }

  /** Host of an `rtsp://` URL, or "" when it is not one. The host doubles as the device key, so a
   *  stream URL is enough to add a source — the user never types the address twice. */
  function hostFromRtsp(url: string): string {
    const raw = url.trim();
    if (!raw.toLowerCase().startsWith("rtsp://")) return "";
    try {
      return new URL(raw).hostname;
    } catch {
      return "";
    }
  }

  const manualRtspHost = $derived(hostFromRtsp(manualRtspUrl));
  const isRtspMode = $derived(manualMode === "rtsp");

  function onvifFieldsValid(): boolean {
    return manualIp.trim().length > 0 && manualPort.trim().length > 0 && manualUser.trim().length > 0;
  }

  const manualValid = $derived(isRtspMode ? manualRtspHost.length > 0 : onvifFieldsValid());
  /** What to call the source in progress/proof copy — the typed IP, or the URL's host. */
  const manualTarget = $derived(isRtspMode ? manualRtspHost : manualIp.trim());

  /** Probe a stream-URL source, allowing for the edge not having collected the URL yet.
   *
   *  Staging the URL wakes the edge's config long-poll, so it normally lands in about a second —
   *  but "normally" is not "always", and a single probe against a box that has not pulled yet fails
   *  as `unreachable`, which reads to the user as a wrong URL. Retry a few times before believing
   *  the verdict. */
  async function probeAfterEdgePull(ip: string): Promise<ProbeResult> {
    let last: ProbeResult = { status: "unreachable", detail: "", snapshot: null };
    for (let attempt = 0; attempt < 4; attempt++) {
      await new Promise((r) => setTimeout(r, attempt === 0 ? 800 : 1500));
      last = await data.probeCredentials(ip, manualUser, manualPass);
      if (last.status === "ok" || last.status === "auth_failed") return last;
    }
    return last;
  }

  // Add-by-hand converges with the discovered path at saveDevice + the #88 probe: the probe resolves
  // the stream over ONVIF from the xaddr we build, so it validates the port, the creds, and returns a
  // proof frame in one shot. Persist only once the camera proves itself — same rule as the wizard.
  async function saveManual() {
    if (!manualValid) return;
    savingManual = true;
    manualState = "testing";
    manualVerdict = null;
    manualSnapshotUrl = null;
    try {
      const rtsp = isRtspMode ? manualRtspUrl.trim() : "";
      const ip = isRtspMode ? manualRtspHost : manualIp.trim();
      const port = manualPort.trim();
      const device: Device = {
        id: "", // backfilled by ONVIF id on the next successful discovery (IP match)
        ip,
        // No ONVIF endpoint in stream-URL mode — there is nothing at the other end to ask.
        xaddr: isRtspMode ? "" : `http://${ip}:${port}/onvif/device_service`,
        name: manualName.trim() || `Camera ${ip}`,
        username: null,
        password: null,
      };
      await data.saveDevice(device);

      // Order differs by mode, and it matters. ONVIF proves the camera first and saves the
      // credentials only once proven (card #88). Stream-URL mode CANNOT: the probe reaches the
      // camera by way of the URL, so the URL has to be on the edge before the probe runs. Staging
      // it wakes the edge's config long-poll immediately, so the wait is short — but it is a race,
      // hence the retry rather than a single attempt against a box that has not pulled yet.
      if (isRtspMode) {
        await data.setCredentials(ip, manualUser, manualPass, rtsp);
      }
      const res = isRtspMode
        ? await probeAfterEdgePull(ip)
        : await data.probeCredentials(ip, manualUser, manualPass);
      manualVerdict = res;
      if (res.status !== "ok") {
        manualState = "failed";
        return;
      }
      if (res.snapshot) manualSnapshotUrl = `data:image/jpeg;base64,${res.snapshot}`;
      if (!isRtspMode) {
        await data.setCredentials(ip, manualUser, manualPass);
      }
      manualState = "verified";
      await loadDevices();
      await loadEdgeFound();
    } catch (err) {
      console.error("Failed to add camera manually", err);
      manualVerdict = null;
      manualState = "failed";
      toast.error(t.manualAddFailed, { description: String(err) });
    } finally {
      savingManual = false;
    }
  }

  // A wrong ONVIF port looks like "can't reach the camera" to the probe — so on the failure paths that
  // a bad port produces, nudge toward it. On auth_failed the port was clearly fine (the camera
  // answered and rejected creds), so don't muddy that verdict.
  function manualAdvice(v: ProbeResult | null): string {
    const base = verdictAdvice(v);
    if (v && (v.status === "unreachable" || v.status === "timeout" || v.status === "error")) {
      return `${base} ${t.portAdvice(manualPort)}`;
    }
    return base;
  }

  function manualButtonLabel(): string {
    if (savingManual) return t.testing;
    if (manualState === "failed" && canRetry(manualVerdict)) return t.testAgain;
    return t.addAndTest;
  }

  // ---- Delete confirm dialog ----------------------------------------------
  let deleteOpen = $state(false);
  let deleteTarget = $state<Device | null>(null);
  let deleting = $state(false);

  function makeModel(d: Device): string {
    const parts = [d.manufacturer, d.model].filter((p) => p && p.length > 0);
    return parts.length > 0 ? parts.join(" / ") : "—";
  }

  async function loadDevices() {
    loading = true;
    try {
      devices = await data.listDevices();
    } catch (err) {
      console.error("Failed to list devices", err);
      toast.error(t.listFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  }

  async function loadEdgeFound() {
    try {
      const [found, configured, streaming] = await Promise.all([
        data.listDiscovered(),
        data.listConfigured(),
        data.listStreaming().catch(() => [] as StreamingCamera[]),
      ]);
      edgeFound = found;
      configuredIps = new Set(configured);
      streamingIps = new Set(streaming.map((s) => s.ip));
      codecByIp = new Map(
        streaming.filter((s): s is StreamingCamera & { codec: string } => !!s.codec).map((s) => [s.ip, s.codec]),
      );
    } catch {
      edgeFound = [];
    }
  }

  // Ask the edge to re-run ONVIF discovery now (e.g. after a camera changed IP). The edge
  // reconciles moves by stable ONVIF id + pushes corrections back; refresh shortly after.
  let rescanning = $state(false);
  async function rescan() {
    rescanning = true;
    try {
      await data.rescan();
      toast.success(t.rescanRequested, { description: t.rescanRequestedHint });
      setTimeout(() => {
        loadDevices();
        loadEdgeFound();
      }, 4000);
    } catch (err) {
      toast.error(t.rescanFailed, { description: String(err) });
    } finally {
      rescanning = false;
    }
  }

  onMount(() => {
    loadDevices();
    loadEdgeFound();
    // Live-refresh when the chat agent renames/removes a device.
    const unlisten = onEvent("devices-changed", () => loadDevices());
    return () => {
      unlisten.then((un) => un());
    };
  });

  // Promote an edge-discovered camera into the device list (admin adds credentials next).
  async function addEdgeFound(d: EdgeDiscovered) {
    try {
      await data.saveDevice({
        id: d.onvif_id ?? "",
        ip: d.ip,
        xaddr: d.xaddr,
        name: d.name || d.ip,
        manufacturer: d.manufacturer,
        model: d.model,
      });
      toast.success(t.added(d.name || d.ip));
      await loadDevices();
    } catch (err) {
      toast.error(t.addFailed, { description: String(err) });
    }
  }
  // Edge-found cameras not already saved.
  const edgeUnsaved = $derived(edgeFound.filter((d) => !devices.some((s) => s.ip === d.ip)));

  function openDetail(d: Device) {
    detail = d;
    editName = d.name;
    editUser = d.username ?? "";
    editPass = d.password ?? "";
    editStreamMode = d.stream_mode === "unified" ? "unified" : "split";
    resetTest();
    detailOpen = true;
  }

  function resetTest() {
    testState = "idle";
    verdict = null;
    // The proof frame is a data: URL now (the edge returns it inline), so there's no object
    // URL to revoke — assigning null is enough to drop it.
    testSnapshotUrl = null;
  }

  // Test the credentials, and save them ONLY once the camera has proven them (card #88).
  //
  // The order matters more than it looks. Saving first hands unproven creds to the edge, which
  // just fail, it overwrote a working camera's config and started an auth storm. Consumer cameras
  // (Tapo, Reolink) lock their RTSP server after such a burst and then refuse *everyone* until
  // they're power-cycled. Probe first, once; persist only on `ok`.
  async function saveAndTest() {
    if (!detail || !editUser.trim()) return;
    savingDetail = true;
    resetTest();
    testState = "testing";
    try {
      const res = await data.probeCredentials(detail.ip, editUser, editPass);
      verdict = res;
      if (res.status !== "ok") {
        testState = "failed";
        return; // Nothing is saved — the camera's config is left exactly as it was.
      }
      if (res.snapshot) testSnapshotUrl = `data:image/jpeg;base64,${res.snapshot}`;
      // Proven — now it's safe to persist the name and hand the creds to the edge.
      const device: Device = { ...detail, name: editName.trim() || detail.name, stream_mode: editStreamMode, username: null, password: null };
      await data.saveDevice(device);
      await data.setCredentials(detail.ip, editUser, editPass);
      detail = device;
      testState = "verified";
      await loadDevices();
      await loadEdgeFound();
    } catch (err) {
      console.error("Failed to test device", err);
      verdict = null;
      testState = "failed";
      toast.error(t.testFailed, { description: String(err) });
    } finally {
      savingDetail = false;
    }
  }

  // One cause, one piece of advice. `detail` from the edge is the operator-facing reason and is
  // shown underneath, but the headline has to tell the user what to actually DO.
  function verdictTitle(v: ProbeResult | null): string {
    if (!v) return t.vError;
    const titles: Record<ProbeStatus, string> = {
      ok: t.vOk,
      auth_failed: t.vAuthFailed,
      locked: t.vLocked,
      not_found: t.vNotFound,
      unreachable: t.vUnreachable,
      timeout: t.vTimeout,
      error: t.vError,
    };
    return titles[v.status] ?? titles.error;
  }

  function verdictAdvice(v: ProbeResult | null): string {
    if (!v) return t.aNone;
    const advice: Record<ProbeStatus, string> = {
      ok: "",
      auth_failed: t.aAuthFailed,
      locked: t.aLocked,
      not_found: t.aNotFound,
      unreachable: t.aUnreachable,
      timeout: t.aTimeout,
      error: t.aError,
    };
    return advice[v.status] ?? advice.error;
  }

  // Offer "Save without testing" ONLY when the test couldn't reach any edge at all (verdict is null —
  // the probe infra failed, e.g. no box online, or the only edge is on-device and
  // localhost-bound). NOT when the camera itself answered and rejected the creds (auth_failed/locked)
  // — bypassing those would hand a bad password to the edge and risk the very auth-lockout the
  // doesn't apply to it; it just collects the creds on its next direct poll.
  function canSaveUntested(): boolean {
    return testState === "failed" && !verdict;
  }
  // Retrying a rejection or a lockout is exactly what causes a lockout, so don't invite it.
  function canRetry(v: ProbeResult | null): boolean {
    if (!v) return true;
    return v.status !== "auth_failed" && v.status !== "locked";
  }

  function testButtonLabel(): string {
    if (savingDetail) return t.testing;
    if (testState === "failed" && canRetry(verdict)) return t.testAgain;
    return t.testAndSave;
  }

  async function saveDetail() {
    if (!detail) return;
    savingDetail = true;
    try {
      // Save the name + stream mode; relay credentials to the edge (never stored in the cloud).
      const device: Device = { ...detail, name: editName.trim() || detail.name, stream_mode: editStreamMode, username: null, password: null };
      await data.saveDevice(device);
      if (editUser.length > 0) {
        await data.setCredentials(detail.ip, editUser, editPass);
        toast.success(t.savedWithCreds);
      } else {
        toast.success(t.savedDevice);
      }
      detail = device;
      await loadDevices();
      await loadEdgeFound(); // refresh the "configured" flags
    } catch (err) {
      console.error("Failed to save device", err);
      toast.error(t.saveFailed, { description: String(err) });
    } finally {
      savingDetail = false;
    }
  }

  function confirmDelete(d: Device) {
    deleteTarget = d;
    deleteOpen = true;
  }

  async function doDelete() {
    if (!deleteTarget) return;
    deleting = true;
    try {
      await data.deleteDevice(deleteTarget.ip);
      toast.success(t.removed(deleteTarget.name));
      deleteOpen = false;
      deleteTarget = null;
      await loadDevices();
    } catch (err) {
      console.error("Failed to delete device", err);
      toast.error(t.deleteFailed, { description: String(err) });
    } finally {
      deleting = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={Cpu}
  >
    {#snippet actions()}
      <Button variant="outline" onclick={rescan} disabled={rescanning} title={t.rescanTitle}>
        <Radar class="size-4" /> {rescanning ? t.rescanning : t.rescan}
      </Button>
      <Button variant="outline" onclick={openManual} title={t.addManuallyTitle}>
        <Plus class="size-4" /> {t.addManually}
      </Button>
    {/snippet}
  </PageHeader>

  <!-- Saved devices -->
  <Card.Root>
    <Card.Header>
      <Card.Title>{t.savedTitle}</Card.Title>
      <Card.Description>{t.savedDescription}</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label={t.loadingDevices} />
      {:else if devices.length === 0}
        <EmptyState title={t.emptyTitle} description={t.emptyDescription} />
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>{t.colName}</Table.Head>
              <Table.Head>{t.colIp}</Table.Head>
              <Table.Head>{t.colMakeModel}</Table.Head>
              <Table.Head>{t.colCredentials}</Table.Head>
              <Table.Head>{t.colStatus}</Table.Head>
              <Table.Head class="text-right">{t.colActions}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each devices as d (d.ip)}
              <Table.Row class="transition-colors hover:bg-primary/5">
                <Table.Cell class="font-medium">
                  <span class="flex items-center gap-2" title={cameraTitle(d)}>
                    <StatusDot state={cameraState(d)} />
                    {d.name}
                    {#if codecByIp.get(d.ip)}
                      <Badge
                        variant="outline"
                        class="font-mono text-[10px] font-normal text-muted-foreground"
                        title={t.codecTitle}
                      >
                        {codecByIp.get(d.ip)}
                      </Badge>
                    {/if}
                  </span>
                </Table.Cell>
                <Table.Cell class="tabular-nums text-muted-foreground">{d.ip}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{makeModel(d)}</Table.Cell>
                <Table.Cell>
                  {#if isConfigured(d)}
                    <Badge
                      variant="outline"
                      class="border-primary/30 bg-primary/10 text-primary"
                    >
                      {t.configured}
                    </Badge>
                  {:else}
                    <Badge variant="outline">{t.setupNeeded}</Badge>
                  {/if}
                </Table.Cell>
                <Table.Cell>
                  <CameraStatus
                    status={d.probe_status}
                    detail={d.probe_detail}
                    checkedAt={d.probe_at}
                    compact
                  />
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      title={t.viewLogs}
                      onclick={() => goto(`/logs?device=${encodeURIComponent(d.ip)}`)}
                    >
                      <ScrollText class="size-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      title={t.editSnapshot}
                      onclick={() => openDetail(d)}
                    >
                      <Pencil class="size-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      title={t.delete}
                      onclick={() => confirmDelete(d)}
                    >
                      <Trash2 class="size-4" />
                    </Button>
                  </div>
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>

  <!-- Found on your edge (cloud: cameras the edge discovered on its LAN) -->
  {#if edgeUnsaved.length > 0}
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><Radar class="size-4 text-primary" /> {t.foundTitle}</Card.Title>
        <Card.Description>{t.foundDescription}</Card.Description>
      </Card.Header>
      <Card.Content>
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>{t.colName}</Table.Head>
              <Table.Head>{t.colIp}</Table.Head>
              <Table.Head>{t.colMakeModel}</Table.Head>
              <Table.Head class="text-right">{t.colAction}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each edgeUnsaved as d (d.ip)}
              <Table.Row>
                <Table.Cell class="font-medium">{d.name || d.ip}</Table.Cell>
                <Table.Cell class="tabular-nums text-muted-foreground">{d.ip}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{[d.manufacturer, d.model].filter(Boolean).join(" ") || "—"}</Table.Cell>
                <Table.Cell class="text-right">
                  <Button variant="outline" size="sm" onclick={() => addEdgeFound(d)}>
                    <Plus class="size-4" /> {t.add}
                  </Button>
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      </Card.Content>
    </Card.Root>
  {/if}

</div>

<!-- Device detail dialog -->
<Dialog.Root bind:open={detailOpen}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{detail?.name ?? t.device}</Dialog.Title>
      <Dialog.Description>
        {detail?.ip}{detail?.xaddr ? ` · ${detail.xaddr}` : ""}
      </Dialog.Description>
    </Dialog.Header>

    {#if detail}
      <div class="space-y-4">
        <div class="space-y-2">
          <Label for="detail-name">{t.colName}</Label>
          <Input id="detail-name" bind:value={editName} placeholder={t.displayName} />
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-2">
            <Label for="detail-user">{t.username}</Label>
            <Input id="detail-user" bind:value={editUser} placeholder="admin" autocomplete="off" />
          </div>
          <div class="space-y-2">
            <Label for="detail-pass">{t.password}</Label>
            <Input id="detail-pass" type="password" bind:value={editPass} placeholder="••••••" autocomplete="off" />
          </div>
        </div>

        <p class="text-xs text-muted-foreground">{t.credsHint}</p>

        <!-- Stream mode (#120): Split (sub + main) vs Unified (main only, HD faces). -->
        <div class="space-y-2 rounded-lg border border-border bg-well p-3">
          <Label class="text-sm font-medium">{t.streamMode}</Label>
          <div class="grid gap-2 sm:grid-cols-2">
            <button
              type="button"
              class="rounded-md border px-3 py-2 text-left text-sm transition-colors {editStreamMode === 'split' ? 'border-primary bg-primary/10' : 'border-border hover:bg-primary/5'}"
              onclick={() => (editStreamMode = "split")}
            >
              <span class="font-medium">{t.split}</span>
              <span class="mt-0.5 block text-[11px] leading-snug text-muted-foreground">{t.splitHint}</span>
            </button>
            <button
              type="button"
              class="rounded-md border px-3 py-2 text-left text-sm transition-colors {editStreamMode === 'unified' ? 'border-primary bg-primary/10' : 'border-border hover:bg-primary/5'}"
              onclick={() => (editStreamMode = "unified")}
            >
              <span class="font-medium">{t.unified}</span>
              <span class="mt-0.5 block text-[11px] leading-snug text-muted-foreground">{t.unifiedHint}</span>
            </button>
          </div>
          {#if editStreamMode === "unified"}
            <p class="text-[11px] leading-snug text-muted-foreground">{t.unifiedNote}</p>
          {/if}
        </div>

        {#if testState !== "idle"}
          <div class="rounded-lg border border-border bg-well p-3">
            {#if testState === "testing"}
              <div class="flex items-center gap-3 text-sm text-muted-foreground">
                <Busy />
                {t.provingCreds(detail.ip)}
              </div>
            {:else if testState === "verified"}
              <div class="space-y-2">
                <p class="flex items-center gap-2 text-sm font-medium text-primary">
                  <Check class="size-4" /> {verdictTitle(verdict)}
                </p>
                {#if testSnapshotUrl}
                  <img src={testSnapshotUrl} alt={t.liveFrameAlt(detail.ip)} class="w-full rounded-md border border-border" />
                {/if}
              </div>
            {:else if testState === "failed"}
              <div class="space-y-1 text-sm">
                <p class="flex items-center gap-2 font-medium text-destructive">
                  <XCircle class="size-4" /> {verdictTitle(verdict)}
                </p>
                <p class="text-muted-foreground">{verdictAdvice(verdict)}</p>
                {#if verdict?.detail}
                  <p class="pt-1 text-xs text-muted-foreground/70">{t.cameraSaid(verdict.detail)}</p>
                {/if}
              </div>
            {/if}
          </div>
        {/if}

        <div class="flex flex-wrap gap-2">
          {#if editUser.trim()}
            <Button onclick={saveAndTest} disabled={savingDetail}>
              {testButtonLabel()}
            </Button>
            {#if canSaveUntested()}
              <Button variant="outline" onclick={saveDetail} disabled={savingDetail} title={t.saveWithoutTestingTitle}>
                {savingDetail ? t.saving : t.saveWithoutTesting}
              </Button>
            {/if}
          {:else}
            <Button onclick={saveDetail} disabled={savingDetail}>
              {savingDetail ? t.saving : c.save}
            </Button>
          {/if}
        </div>
        {#if canSaveUntested()}
          <p class="text-xs text-muted-foreground">{t.untestedHint}</p>
        {/if}
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirm dialog -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t.deleteTitle}</Dialog.Title>
      <Dialog.Description>
        {t.deleteBefore}<span class="font-medium text-foreground">{deleteTarget?.name}</span>{t.deleteAfter(deleteTarget?.ip ?? "")}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>
        {c.cancel}
      </Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>
        {deleting ? t.deleting : t.delete}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Add camera manually (card #97) -->
<Dialog.Root bind:open={manualOpen}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{t.manualTitle}</Dialog.Title>
      <Dialog.Description>{t.manualDescription}</Dialog.Description>
    </Dialog.Header>

    <div class="space-y-4">
      <div class="space-y-2">
        <Label for="m-name">{t.colName}</Label>
        <Input id="m-name" bind:value={manualName} placeholder={t.namePlaceholder} />
      </div>

      <div class="space-y-2">
        <Label>{t.sourceMode}</Label>
        <div class="grid grid-cols-2 gap-2">
          <Button
            type="button"
            variant={isRtspMode ? "outline" : "default"}
            onclick={() => (manualMode = "onvif")}
          >
            {t.sourceOnvif}
          </Button>
          <Button
            type="button"
            variant={isRtspMode ? "default" : "outline"}
            onclick={() => (manualMode = "rtsp")}
          >
            {t.sourceRtsp}
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">
          {isRtspMode ? t.sourceRtspHint : t.sourceOnvifHint}
        </p>
      </div>

      {#if isRtspMode}
        <div class="space-y-2">
          <Label for="m-rtsp">{t.rtspUrl}</Label>
          <Input
            id="m-rtsp"
            bind:value={manualRtspUrl}
            placeholder={t.rtspUrlPlaceholder}
            autocomplete="off"
            spellcheck="false"
          />
          {#if manualRtspUrl.trim().length > 0 && manualRtspHost.length === 0}
            <p class="text-xs text-destructive">{t.rtspUrlInvalid}</p>
          {/if}
        </div>
      {:else}
        <div class="grid grid-cols-[1fr_auto] gap-3">
          <div class="space-y-2">
            <Label for="m-ip">{t.ipAddress}</Label>
            <Input id="m-ip" bind:value={manualIp} placeholder="192.168.1.187" />
          </div>
          <div class="space-y-2">
            <Label for="m-port">{t.onvifPort}</Label>
            <Input id="m-port" bind:value={manualPort} placeholder="2020" class="w-28" />
          </div>
        </div>
      {/if}

      <div class="grid grid-cols-2 gap-3">
        <div class="space-y-2">
          <Label for="m-user">{t.username}</Label>
          <Input id="m-user" bind:value={manualUser} placeholder="admin" autocomplete="off" />
        </div>
        <div class="space-y-2">
          <Label for="m-pass">{t.password}</Label>
          <Input id="m-pass" type="password" bind:value={manualPass} placeholder="••••••" autocomplete="off" />
        </div>
      </div>

      {#if isRtspMode}
        <p class="text-xs text-muted-foreground">{t.rtspUrlHint}</p>
      {:else}
        <p class="text-xs text-muted-foreground">
          {t.portHintBefore}<span class="font-medium">2020</span>{t.portHintMiddle}<span
            class="font-medium">80</span
          >{t.portHintAfter}
        </p>
      {/if}

      {#if manualState !== "idle"}
        <div class="rounded-lg border border-border bg-well p-3">
          {#if manualState === "testing"}
            <div class="flex items-center gap-3 text-sm text-muted-foreground">
              <Busy />
              {t.provingDetails(manualTarget)}
            </div>
          {:else if manualState === "verified"}
            <div class="space-y-2">
              <p class="flex items-center gap-2 text-sm font-medium text-primary">
                <Check class="size-4" /> {verdictTitle(manualVerdict)}
              </p>
              {#if manualSnapshotUrl}
                <img src={manualSnapshotUrl} alt={t.liveFrameAlt(manualTarget)} class="w-full rounded-md border border-border" />
              {/if}
            </div>
          {:else if manualState === "failed"}
            <div class="space-y-1 text-sm">
              <p class="flex items-center gap-2 font-medium text-destructive">
                <XCircle class="size-4" /> {verdictTitle(manualVerdict)}
              </p>
              <p class="text-muted-foreground">{manualAdvice(manualVerdict)}</p>
              {#if manualVerdict?.detail}
                <p class="pt-1 text-xs text-muted-foreground/70">{t.cameraSaid(manualVerdict.detail)}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <Dialog.Footer>
      {#if manualState === "verified"}
        <Button onclick={() => (manualOpen = false)}>{t.done}</Button>
      {:else}
        <Button variant="outline" onclick={() => (manualOpen = false)} disabled={savingManual}>{c.cancel}</Button>
        <Button onclick={saveManual} disabled={!manualValid || savingManual}>{manualButtonLabel()}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
