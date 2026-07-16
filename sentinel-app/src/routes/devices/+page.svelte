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
  let savingDetail = $state(false);
  // Credential test (card #88): the camera proves the creds before we save them.
  type TestState = "idle" | "testing" | "verified" | "failed";
  let testState = $state<TestState>("idle");
  let testSnapshotUrl = $state<string | null>(null);
  let verdict = $state<ProbeResult | null>(null);

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
      const device: Device = { ...detail, name: editName.trim() || detail.name, username: null, password: null };
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
      // Save the name only; relay credentials to the edge (never stored in the cloud).
      const device: Device = { ...detail, name: editName.trim() || detail.name, username: null, password: null };
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
            <Input id="detail-user" bind:value={editUser} placeholder="admin" />
          </div>
          <div class="space-y-2">
            <Label for="detail-pass">Password</Label>
            <Input id="detail-pass" type="password" bind:value={editPass} placeholder="••••••" />
          </div>
        </div>

        <p class="text-xs text-muted-foreground">
          Set the camera's credentials — your edge resolves the stream on its own network. The RTSP URL stays on the edge.
        </p>

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
          {:else}
            <Button onclick={saveDetail} disabled={savingDetail}>
              {savingDetail ? "Saving…" : "Save"}
            </Button>
          {/if}
        </div>
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
