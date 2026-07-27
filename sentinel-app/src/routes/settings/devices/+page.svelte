<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Cpu from "@lucide/svelte/icons/cpu";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { data, onEvent, type EdgeDiscovered, type ProbeResult, type ProbeStatus } from "$lib/data";
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
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import Check from "@lucide/svelte/icons/check";
  import XCircle from "@lucide/svelte/icons/circle-x";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import type { Device } from "$lib/types";

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

  // The status dot, using what the CLOUD actually knows (it never holds the RTSP URL — that's
  // edge-local — so the old rtsp_url check was always gray):
  //   online (green) = the edge resolved a working stream (valid creds + camera reachable)
  //   warn   (amber) = creds set but no stream yet (wrong creds / camera down / still resolving),
  //                    or the camera is visible on the LAN but has no credentials yet
  //   offline (gray) = not set up and not visible
  function cameraState(d: Device): "online" | "warn" | "offline" {
    if (streamingIps.has(d.ip)) return "online";
    if (isConfigured(d) || discoveredIps.has(d.ip)) return "warn";
    return "offline";
  }
  function cameraTitle(d: Device): string {
    if (streamingIps.has(d.ip)) return "Online — the edge has a working stream (credentials valid, camera reachable)";
    const onLan = discoveredIps.has(d.ip);
    const configured = isConfigured(d);
    if (configured && onLan) return "Credentials set and camera visible, but no stream yet — check credentials, or it's still resolving";
    if (configured) return "Credentials set, but no stream and not visible on the network (offline, or its IP changed — try Rescan)";
    if (onLan) return "Visible on the network — set its credentials to start streaming";
    return "Not set up, and not visible on the network";
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
  let manualOpen = $state(false);
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

  const manualValid = $derived(
    manualIp.trim().length > 0 && manualPort.trim().length > 0 && manualUser.trim().length > 0,
  );

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
      const ip = manualIp.trim();
      const port = manualPort.trim();
      const device: Device = {
        id: "", // backfilled by ONVIF id on the next successful discovery (IP match)
        ip,
        xaddr: `http://${ip}:${port}/onvif/device_service`,
        name: manualName.trim() || `Camera ${ip}`,
        username: null,
        password: null,
      };
      await data.saveDevice(device);
      const res = await data.probeCredentials(ip, manualUser, manualPass);
      manualVerdict = res;
      if (res.status !== "ok") {
        manualState = "failed";
        return;
      }
      if (res.snapshot) manualSnapshotUrl = `data:image/jpeg;base64,${res.snapshot}`;
      await data.setCredentials(ip, manualUser, manualPass);
      manualState = "verified";
      await loadDevices();
      await loadEdgeFound();
    } catch (err) {
      console.error("Failed to add camera manually", err);
      manualVerdict = null;
      manualState = "failed";
      toast.error("Couldn't add the camera", { description: String(err) });
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
      return `${base} If the camera is reachable but this keeps failing, double-check the ONVIF port — it's ${manualPort} here; Tapo uses 2020, many cameras use 80.`;
    }
    return base;
  }

  function manualButtonLabel(): string {
    if (savingManual) return "Testing…";
    if (manualState === "failed" && canRetry(manualVerdict)) return "Test again";
    return "Add & test";
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
      toast.error("Failed to load devices", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  async function loadEdgeFound() {
    try {
      const [found, configured, streaming] = await Promise.all([
        data.listDiscovered(),
        data.listConfigured(),
        data.listStreaming().catch(() => [] as string[]),
      ]);
      edgeFound = found;
      configuredIps = new Set(configured);
      streamingIps = new Set(streaming);
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
      toast.success("Rescan requested", {
        description: "Your edge box is re-running discovery — moved cameras self-heal by ONVIF id.",
      });
      setTimeout(() => {
        loadDevices();
        loadEdgeFound();
      }, 4000);
    } catch (err) {
      toast.error("Rescan failed", { description: String(err) });
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
      toast.success(`Added ${d.name || d.ip} — open it to add credentials.`);
      await loadDevices();
    } catch (err) {
      toast.error("Failed to add camera", { description: String(err) });
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
  // writes them into go2rtc's config and points a reconnect loop at the camera — so a typo didn't
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
      toast.error("Couldn't test the camera", { description: String(err) });
    } finally {
      savingDetail = false;
    }
  }

  // One cause, one piece of advice. `detail` from the edge is the operator-facing reason and is
  // shown underneath, but the headline has to tell the user what to actually DO.
  function verdictTitle(v: ProbeResult | null): string {
    if (!v) return "Couldn't test this camera.";
    const titles: Record<ProbeStatus, string> = {
      ok: "Camera verified — the credentials work.",
      auth_failed: "The camera rejected these credentials.",
      locked: "The camera has locked its video stream.",
      not_found: "Signed in, but found no video stream.",
      unreachable: "Couldn't reach the camera.",
      timeout: "The camera didn't answer.",
      error: "Couldn't test this camera.",
    };
    return titles[v.status] ?? titles.error;
  }

  function verdictAdvice(v: ProbeResult | null): string {
    if (!v) return "Something went wrong before we could reach the camera. Nothing was saved.";
    const advice: Record<ProbeStatus, string> = {
      ok: "",
      auth_failed:
        "Double-check the username and password. Many cameras need a separate account you create in their own app, not your cloud login. Nothing was saved.",
      locked:
        "It's refusing every connection, not just ours — cameras do this after repeated failed logins. Unplug it for about 30 seconds, then test again. Your saved credentials weren't changed.",
      not_found:
        "The credentials are right, but the camera didn't offer a stream at the expected path. It may need RTSP enabled in its own settings.",
      unreachable:
        "Nothing answered at this address. Check the camera is powered on and on the same network as your box — if its IP changed, try Rescan.",
      timeout:
        "It accepted the connection but never replied. It may be busy or half-crashed; a power cycle usually clears it.",
      error: "Nothing was saved.",
    };
    return advice[v.status] ?? advice.error;
  }

  // Offer "Save without testing" ONLY when the test couldn't reach any edge at all (verdict is null —
  // the probe infra failed, e.g. no box online over the tailnet, or the only edge is on-device and
  // localhost-bound). NOT when the camera itself answered and rejected the creds (auth_failed/locked)
  // — bypassing those would hand a bad password to the edge and risk the very auth-lockout the
  // test-first flow exists to avoid. A localhost box (on-device edge) has no go2rtc, so that risk
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
    if (savingDetail) return "Testing…";
    if (testState === "failed" && canRetry(verdict)) return "Test again";
    return "Test & save";
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
        toast.success("Saved — credentials sent to your edge.");
      } else {
        toast.success("Device saved.");
      }
      detail = device;
      await loadDevices();
      await loadEdgeFound(); // refresh the "configured" flags
    } catch (err) {
      console.error("Failed to save device", err);
      toast.error("Failed to save device", { description: String(err) });
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
      toast.success(`Removed ${deleteTarget.name}.`);
      deleteOpen = false;
      deleteTarget = null;
      await loadDevices();
    } catch (err) {
      console.error("Failed to delete device", err);
      toast.error("Failed to delete device", { description: String(err) });
    } finally {
      deleting = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Devices"
    description="Save and manage the cameras your edge appliance finds on its local network."
    icon={Cpu}
  >
    {#snippet actions()}
      <Button variant="outline" onclick={rescan} disabled={rescanning} title="Re-run ONVIF discovery on your edge box now (fixes cameras that changed IP)">
        <Radar class="size-4" /> {rescanning ? "Rescanning…" : "Rescan"}
      </Button>
      <Button variant="outline" onclick={openManual} title="Add a camera by its IP when discovery can't find it (e.g. the network blocks discovery)">
        <Plus class="size-4" /> Add manually
      </Button>
    {/snippet}
  </PageHeader>

  <!-- Saved devices -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Saved devices</Card.Title>
      <Card.Description>Cameras you've added, with stored credentials.</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label="Loading devices…" />
      {:else if devices.length === 0}
        <EmptyState
          title="No devices yet"
          description="When your edge appliance discovers cameras on its network, they'll appear below to add."
        />
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head>IP</Table.Head>
              <Table.Head>Make / Model</Table.Head>
              <Table.Head>Credentials</Table.Head>
              <Table.Head class="text-right">Actions</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each devices as d (d.ip)}
              <Table.Row class="transition-colors hover:bg-primary/5">
                <Table.Cell class="font-medium">
                  <span class="flex items-center gap-2" title={cameraTitle(d)}>
                    <StatusDot state={cameraState(d)} />
                    {d.name}
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
                      Configured
                    </Badge>
                  {:else}
                    <Badge variant="outline">Setup needed</Badge>
                  {/if}
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      title="View this camera's edge logs"
                      onclick={() => goto(`/logs?device=${encodeURIComponent(d.ip)}`)}
                    >
                      <ScrollText class="size-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      title="Edit / snapshot"
                      onclick={() => openDetail(d)}
                    >
                      <Pencil class="size-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      title="Delete"
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
        <Card.Title class="flex items-center gap-2"><Radar class="size-4 text-primary" /> Found on your edge</Card.Title>
        <Card.Description>Cameras your edge appliance discovered on its network. Add one, then open it to set credentials.</Card.Description>
      </Card.Header>
      <Card.Content>
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head>IP</Table.Head>
              <Table.Head>Make / Model</Table.Head>
              <Table.Head class="text-right">Action</Table.Head>
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
                    <Plus class="size-4" /> Add
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
      <Dialog.Title>{detail?.name ?? "Device"}</Dialog.Title>
      <Dialog.Description>
        {detail?.ip}{detail?.xaddr ? ` · ${detail.xaddr}` : ""}
      </Dialog.Description>
    </Dialog.Header>

    {#if detail}
      <div class="space-y-4">
        <div class="space-y-2">
          <Label for="detail-name">Name</Label>
          <Input id="detail-name" bind:value={editName} placeholder="Display name" />
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-2">
            <Label for="detail-user">Username</Label>
            <Input id="detail-user" bind:value={editUser} placeholder="admin" autocomplete="off" />
          </div>
          <div class="space-y-2">
            <Label for="detail-pass">Password</Label>
            <Input id="detail-pass" type="password" bind:value={editPass} placeholder="••••••" autocomplete="off" />
          </div>
        </div>

        <p class="text-xs text-muted-foreground">
          Set the camera's credentials — your edge resolves the stream on its own network. The RTSP URL stays on the edge.
        </p>

        <!-- Stream mode (#120): Split (sub + main) vs Unified (main only, HD faces). -->
        <div class="space-y-2 rounded-lg border border-border bg-well p-3">
          <Label class="text-sm font-medium">Stream mode</Label>
          <div class="grid gap-2 sm:grid-cols-2">
            <button
              type="button"
              class="rounded-md border px-3 py-2 text-left text-sm transition-colors {editStreamMode === 'split' ? 'border-primary bg-primary/10' : 'border-border hover:bg-primary/5'}"
              onclick={() => (editStreamMode = "split")}
            >
              <span class="font-medium">Split</span>
              <span class="mt-0.5 block text-[11px] leading-snug text-muted-foreground">Substream for preview + motion, main for HD/recording. Default.</span>
            </button>
            <button
              type="button"
              class="rounded-md border px-3 py-2 text-left text-sm transition-colors {editStreamMode === 'unified' ? 'border-primary bg-primary/10' : 'border-border hover:bg-primary/5'}"
              onclick={() => (editStreamMode = "unified")}
            >
              <span class="font-medium">Unified (HD)</span>
              <span class="mt-0.5 block text-[11px] leading-snug text-muted-foreground">Main stream only — sharp HD faces, no substream. One camera session.</span>
            </button>
          </div>
          {#if editStreamMode === "unified"}
            <p class="text-[11px] leading-snug text-muted-foreground">
              Motion still runs (on a downscaled copy of the main frame). Use this when the camera can't serve two streams at once, or when you want HD faces.
            </p>
          {/if}
        </div>

        {#if testState !== "idle"}
          <div class="rounded-lg border border-border bg-well p-3">
            {#if testState === "testing"}
              <div class="flex items-center gap-3 text-sm text-muted-foreground">
                <Busy />
                Asking {detail.ip} to prove these credentials… nothing is saved until it does.
              </div>
            {:else if testState === "verified"}
              <div class="space-y-2">
                <p class="flex items-center gap-2 text-sm font-medium text-primary">
                  <Check class="size-4" /> {verdictTitle(verdict)}
                </p>
                {#if testSnapshotUrl}
                  <img src={testSnapshotUrl} alt="Live frame from {detail.ip}" class="w-full rounded-md border border-border" />
                {/if}
              </div>
            {:else if testState === "failed"}
              <div class="space-y-1 text-sm">
                <p class="flex items-center gap-2 font-medium text-destructive">
                  <XCircle class="size-4" /> {verdictTitle(verdict)}
                </p>
                <p class="text-muted-foreground">{verdictAdvice(verdict)}</p>
                {#if verdict?.detail}
                  <p class="pt-1 text-xs text-muted-foreground/70">The camera said: {verdict.detail}</p>
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
              <Button variant="outline" onclick={saveDetail} disabled={savingDetail} title="Send these credentials to the box without a live camera test — for a box that can't be probed (e.g. an on-device edge with no tailnet).">
                {savingDetail ? "Saving…" : "Save without testing"}
              </Button>
            {/if}
          {:else}
            <Button onclick={saveDetail} disabled={savingDetail}>
              {savingDetail ? "Saving…" : "Save"}
            </Button>
          {/if}
        </div>
        {#if canSaveUntested()}
          <p class="text-xs text-muted-foreground">
            Couldn't reach a box to test these credentials — no edge is online over the tailnet. If you're
            provisioning an on-device edge (it polls directly, not over the tailnet), you can save the
            credentials without testing and it'll collect them on its next poll.
          </p>
        {/if}
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirm dialog -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Delete device?</Dialog.Title>
      <Dialog.Description>
        Remove <span class="font-medium text-foreground">{deleteTarget?.name}</span>
        ({deleteTarget?.ip}) and its saved credentials. This can't be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>
        Cancel
      </Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>
        {deleting ? "Deleting…" : "Delete"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Add camera manually (card #97) -->
<Dialog.Root bind:open={manualOpen}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>Add a camera by hand</Dialog.Title>
      <Dialog.Description>
        For a camera your box can reach but can't discover — usually when the network blocks camera
        discovery. Enter its address and credentials; your edge resolves the stream the same way.
      </Dialog.Description>
    </Dialog.Header>

    <div class="space-y-4">
      <div class="space-y-2">
        <Label for="m-name">Name</Label>
        <Input id="m-name" bind:value={manualName} placeholder="e.g. Hall" />
      </div>
      <div class="grid grid-cols-[1fr_auto] gap-3">
        <div class="space-y-2">
          <Label for="m-ip">IP address</Label>
          <Input id="m-ip" bind:value={manualIp} placeholder="192.168.1.187" />
        </div>
        <div class="space-y-2">
          <Label for="m-port">ONVIF port</Label>
          <Input id="m-port" bind:value={manualPort} placeholder="2020" class="w-28" />
        </div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="space-y-2">
          <Label for="m-user">Username</Label>
          <Input id="m-user" bind:value={manualUser} placeholder="admin" autocomplete="off" />
        </div>
        <div class="space-y-2">
          <Label for="m-pass">Password</Label>
          <Input id="m-pass" type="password" bind:value={manualPass} placeholder="••••••" autocomplete="off" />
        </div>
      </div>
      <p class="text-xs text-muted-foreground">
        The ONVIF port is the camera's control port — Tapo uses <span class="font-medium">2020</span>,
        many others use <span class="font-medium">80</span>. The camera's credentials are usually a
        separate account you create in its own app, not your cloud login.
      </p>

      {#if manualState !== "idle"}
        <div class="rounded-lg border border-border bg-well p-3">
          {#if manualState === "testing"}
            <div class="flex items-center gap-3 text-sm text-muted-foreground">
              <Busy />
              Asking {manualIp} to prove these details… nothing is saved until it does.
            </div>
          {:else if manualState === "verified"}
            <div class="space-y-2">
              <p class="flex items-center gap-2 text-sm font-medium text-primary">
                <Check class="size-4" /> {verdictTitle(manualVerdict)}
              </p>
              {#if manualSnapshotUrl}
                <img src={manualSnapshotUrl} alt="Live frame from {manualIp}" class="w-full rounded-md border border-border" />
              {/if}
            </div>
          {:else if manualState === "failed"}
            <div class="space-y-1 text-sm">
              <p class="flex items-center gap-2 font-medium text-destructive">
                <XCircle class="size-4" /> {verdictTitle(manualVerdict)}
              </p>
              <p class="text-muted-foreground">{manualAdvice(manualVerdict)}</p>
              {#if manualVerdict?.detail}
                <p class="pt-1 text-xs text-muted-foreground/70">The camera said: {manualVerdict.detail}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <Dialog.Footer>
      {#if manualState === "verified"}
        <Button onclick={() => (manualOpen = false)}>Done</Button>
      {:else}
        <Button variant="outline" onclick={() => (manualOpen = false)} disabled={savingManual}>Cancel</Button>
        <Button onclick={saveManual} disabled={!manualValid || savingManual}>{manualButtonLabel()}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
