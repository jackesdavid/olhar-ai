<script lang="ts">
  import { onMount } from "svelte";
  import { data, onEvent, type EdgeDiscovered } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Radar from "@lucide/svelte/icons/radar";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
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

  // ---- Detail dialog (edit name + credentials, test, snapshot) -------------
  let detailOpen = $state(false);
  let detail = $state<Device | null>(null);
  let editName = $state("");
  let editUser = $state("");
  let editPass = $state("");
  let savingDetail = $state(false);

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
      [edgeFound, configuredIps] = await Promise.all([
        data.listDiscovered(),
        data.listConfigured().then((ips) => new Set(ips)),
      ]);
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

  function openDetail(d: Device) {
    detail = d;
    editName = d.name;
    editUser = d.username ?? "";
    editPass = d.password ?? "";
    detailOpen = true;
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
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Devices</h1>
      <p class="text-sm text-muted-foreground">
        Save and manage the cameras your edge appliance finds on its local network.
      </p>
    </div>
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
                  <span class="flex items-center gap-2">
                    <StatusDot state={d.rtsp_url && d.rtsp_url.trim() ? "online" : "offline"} />
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

        <div class="flex flex-wrap gap-2">
          <Button onclick={saveDetail} disabled={savingDetail}>
            {savingDetail ? "Saving…" : "Save"}
          </Button>
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
