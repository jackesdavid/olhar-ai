<script lang="ts">
  import { onMount } from "svelte";
  import { data, onEvent, isTauri, type EdgeDiscovered } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Radar from "@lucide/svelte/icons/radar";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Camera from "@lucide/svelte/icons/camera";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import Wifi from "@lucide/svelte/icons/wifi";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import { Separator } from "$lib/components/ui/separator";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import type { Device, DiscoveredDevice } from "$lib/types";

  let devices = $state<Device[]>([]);
  let loading = $state(true);

  let discovered = $state<DiscoveredDevice[]>([]);
  let scanning = $state(false);

  // Cloud mode: cameras the tenant's edge found on its LAN and pushed up.
  let edgeFound = $state<EdgeDiscovered[]>([]);

  // ---- Detail dialog (edit name + credentials, test, snapshot) -------------
  let detailOpen = $state(false);
  let detail = $state<Device | null>(null);
  let editName = $state("");
  let editUser = $state("");
  let editPass = $state("");
  let testing = $state(false);
  let savingDetail = $state(false);
  let capturing = $state(false);
  let snapshotUrl = $state<string | null>(null);

  // ---- Delete confirm dialog ----------------------------------------------
  let deleteOpen = $state(false);
  let deleteTarget = $state<Device | null>(null);
  let deleting = $state(false);

  function label(s: string | null | undefined): string {
    return s && s.length > 0 ? s : "—";
  }

  function makeModel(d: Device | DiscoveredDevice): string {
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
    if (isTauri()) return; // desktop uses the live ONVIF scan below
    try {
      edgeFound = await data.listDiscovered();
    } catch {
      edgeFound = [];
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

  async function scan() {
    if (!isTauri()) return;
    scanning = true;
    discovered = [];
    try {
      const found = await data.discoverDevices();
      discovered = found;
      if (found.length === 0) {
        toast.info("No cameras found on the local network.");
      } else {
        toast.success(`Found ${found.length} camera${found.length === 1 ? "" : "s"}.`);
      }
    } catch (err) {
      console.error("Discovery failed", err);
      toast.error("Discovery failed", { description: String(err) });
    } finally {
      scanning = false;
    }
  }

  const savedIps = $derived(new Set(devices.map((d) => d.ip)));
  const savedByIp = $derived(new Map(devices.map((d) => [d.ip, d])));
  // For a discovered camera, prefer the name you saved it under (less confusing than
  // the camera's advertised name once it's in your list).
  function discoveredName(d: DiscoveredDevice): string {
    return savedByIp.get(d.ip)?.name ?? d.name;
  }

  async function addDiscovered(d: DiscoveredDevice) {
    const device: Device = {
      id: d.id ?? "",
      ip: d.ip,
      xaddr: d.xaddr,
      name: d.name,
      manufacturer: d.manufacturer ?? null,
      model: d.model ?? null,
      username: null,
      password: null,
      rtsp_url: null,
    };
    try {
      await data.saveDevice(device);
      toast.success(`Added ${device.name}.`);
      await loadDevices();
    } catch (err) {
      console.error("Failed to add device", err);
      toast.error("Failed to add device", { description: String(err) });
    }
  }

  function openDetail(d: Device) {
    detail = d;
    editName = d.name;
    editUser = d.username ?? "";
    editPass = d.password ?? "";
    snapshotUrl = null;
    detailOpen = true;
  }

  async function saveDetail() {
    if (!detail) return;
    savingDetail = true;
    const device: Device = {
      ...detail,
      name: editName.trim() || detail.name,
      username: editUser.length > 0 ? editUser : null,
      password: editPass.length > 0 ? editPass : null,
    };
    try {
      await data.saveDevice(device);
      detail = device;
      toast.success("Device saved.");
      await loadDevices();
    } catch (err) {
      console.error("Failed to save device", err);
      toast.error("Failed to save device", { description: String(err) });
    } finally {
      savingDetail = false;
    }
  }

  async function testCreds() {
    if (!detail) return;
    if (!isTauri()) return;
    if (editUser.length === 0) {
      toast.error("Enter a username and password first.");
      return;
    }
    testing = true;
    try {
      // validate_device verifies the creds AND persists them + the resolved
      // rtsp_url onto the saved device (so snapshots and live video work).
      const updated = await data.validateDevice(detail.ip, editUser, editPass);
      detail = updated;
      editUser = updated.username ?? editUser;
      editPass = updated.password ?? editPass;
      toast.success("Credentials verified and saved.", {
        description: updated.rtsp_url ?? undefined,
      });
      await loadDevices();
    } catch (err) {
      console.error("Credential check failed", err);
      toast.error("Credentials rejected", { description: String(err) });
    } finally {
      testing = false;
    }
  }

  async function capture() {
    if (!detail) return;
    capturing = true;
    snapshotUrl = null;
    try {
      snapshotUrl = await data.captureSnapshot(detail.ip);
      toast.success("Snapshot captured.");
    } catch (err) {
      console.error("Snapshot failed", err);
      toast.error("Snapshot failed", { description: String(err) });
    } finally {
      capturing = false;
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
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Devices</h1>
      <p class="text-sm text-muted-foreground">
        Discover, save, and manage ONVIF cameras on the local network.
      </p>
    </div>
    {#if isTauri()}
      <Button onclick={scan} disabled={scanning}>
        {#if scanning}
          <LoaderCircle class="size-4 animate-spin" />
          Scanning…
        {:else}
          <Radar class="size-4" />
          Scan network
        {/if}
      </Button>
    {/if}
  </div>

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
          description="Scan the local network to discover ONVIF cameras nearby."
        >
          {#if isTauri()}
            <Button onclick={scan} disabled={scanning}>
              <Radar class="size-4" /> Scan network
            </Button>
          {/if}
        </EmptyState>
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
                  <span class="flex items-center gap-2">
                    <StatusDot state={d.rtsp_url && d.rtsp_url.trim() ? "online" : "offline"} />
                    {d.name}
                  </span>
                </Table.Cell>
                <Table.Cell class="tabular-nums text-muted-foreground">{d.ip}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{makeModel(d)}</Table.Cell>
                <Table.Cell>
                  {#if d.username}
                    <Badge
                      variant="outline"
                      class="border-primary/30 bg-primary/10 text-primary"
                    >
                      Saved
                    </Badge>
                  {:else}
                    <Badge variant="outline">None</Badge>
                  {/if}
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex justify-end gap-1">
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

  <!-- Discovered devices -->
  {#if scanning || discovered.length > 0}
    <Card.Root>
      <Card.Header>
        <Card.Title>Discovered</Card.Title>
        <Card.Description>Cameras that answered the last discovery sweep.</Card.Description>
      </Card.Header>
      <Card.Content>
        {#if scanning}
          <Loader label="Probing the network…" />
        {:else}
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
              {#each discovered as d (d.ip)}
                <Table.Row>
                  <Table.Cell class="font-medium">
                    {discoveredName(d)}
                    {#if savedByIp.has(d.ip) && savedByIp.get(d.ip)?.name !== d.name}
                      <span class="ml-1 text-xs font-normal text-muted-foreground">· camera: {d.name}</span>
                    {/if}
                  </Table.Cell>
                  <Table.Cell class="tabular-nums text-muted-foreground">{d.ip}</Table.Cell>
                  <Table.Cell class="text-muted-foreground">{makeModel(d)}</Table.Cell>
                  <Table.Cell class="text-right">
                    {#if savedIps.has(d.ip)}
                      <span class="inline-flex items-center gap-1 text-xs text-muted-foreground">
                        <Wifi class="size-3.5" /> Saved
                      </span>
                    {:else}
                      <Button variant="outline" size="sm" onclick={() => addDiscovered(d)}>
                        <Plus class="size-4" /> Add
                      </Button>
                    {/if}
                  </Table.Cell>
                </Table.Row>
              {/each}
            </Table.Body>
          </Table.Root>
        {/if}
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

        <div class="flex flex-wrap gap-2">
          <Button onclick={saveDetail} disabled={savingDetail}>
            {savingDetail ? "Saving…" : "Save"}
          </Button>
          {#if isTauri()}
            <Button variant="outline" onclick={testCreds} disabled={testing}>
              {#if testing}
                <LoaderCircle class="size-4 animate-spin" />
              {:else}
                <KeyRound class="size-4" />
              {/if}
              Test credentials
            </Button>
          {/if}
          <Button variant="outline" onclick={capture} disabled={capturing}>
            {#if capturing}
              <LoaderCircle class="size-4 animate-spin" />
            {:else}
              <Camera class="size-4" />
            {/if}
            Capture snapshot
          </Button>
        </div>

        {#if snapshotUrl}
          <Separator />
          <img
            src={snapshotUrl}
            alt="Snapshot from {detail.name}"
            class="w-full rounded-md border border-border"
          />
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
