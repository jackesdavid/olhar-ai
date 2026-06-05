<script lang="ts">
  // Unit 018 — Map (3D floor plan). A Threlte <Canvas> on the left, a room/camera
  // editor panel on the right. The frontend owns the working copy of the rooms
  // array and saves the whole thing via `save_map`.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import { Canvas } from "@threlte/core";

  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Save from "@lucide/svelte/icons/save";
  import Video from "@lucide/svelte/icons/video";
  import CameraIcon from "@lucide/svelte/icons/camera";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";

  import * as Card from "$lib/components/ui/card";
  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import { Separator } from "$lib/components/ui/separator";

  import MapScene from "$lib/three/MapScene.svelte";
  import type { Device, MapRoom } from "$lib/types";

  const DEFAULT_ROOM_W = 6;
  const DEFAULT_ROOM_H = 4;

  let rooms = $state<MapRoom[]>([]);
  let devices = $state<Device[]>([]);
  let selectedId = $state<number | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let dirty = $state(false);

  // Camera-assignment form state.
  let assignIp = $state<string>("");
  let assignDx = $state(0);
  let assignDy = $state(0);

  // Delete-room confirm dialog.
  let deleteOpen = $state(false);

  const selected = $derived(rooms.find((r) => r.id === selectedId) ?? null);

  const deviceByIp = $derived(new Map(devices.map((d) => [d.ip, d])));
  function deviceName(ip: string): string {
    return deviceByIp.get(ip)?.name ?? ip;
  }

  // Devices not yet assigned to the selected room (candidates for the picker).
  const assignable = $derived(
    selected
      ? devices.filter((d) => !selected.cameras.some((c) => c.ip === d.ip))
      : []
  );
  const assignLabel = $derived(
    assignIp ? deviceName(assignIp) : "Select a camera to assign"
  );

  function nextId(): number {
    return rooms.reduce((m, r) => Math.max(m, r.id), -1) + 1;
  }

  async function load() {
    loading = true;
    try {
      const [rs, ds] = await Promise.all([
        invoke<MapRoom[]>("load_map"),
        invoke<Device[]>("list_devices"),
      ]);
      rooms = rs;
      devices = ds;
      if (rooms.length > 0) selectedId = rooms[0].id;
      dirty = false;
    } catch (err) {
      console.error("Failed to load map", err);
      toast.error("Failed to load map", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(load);

  function addRoom() {
    const id = nextId();
    const room: MapRoom = {
      id,
      name: `Room ${id + 1}`,
      x: 0,
      y: 0,
      w: DEFAULT_ROOM_W,
      h: DEFAULT_ROOM_H,
      cameras: [],
    };
    rooms = [...rooms, room];
    selectedId = id;
    dirty = true;
  }

  function confirmDelete() {
    if (!selected) return;
    rooms = rooms.filter((r) => r.id !== selected.id);
    selectedId = rooms.length > 0 ? rooms[0].id : null;
    dirty = true;
    deleteOpen = false;
  }

  // Mutate the selected room in place (and trigger reactivity + dirty flag).
  function updateSelected(patch: Partial<MapRoom>) {
    if (selectedId === null) return;
    rooms = rooms.map((r) => (r.id === selectedId ? { ...r, ...patch } : r));
    dirty = true;
  }

  function clampInt(v: number, min: number): number {
    return Number.isFinite(v) ? Math.max(min, Math.round(v)) : min;
  }

  function assignCamera() {
    if (!selected || !assignIp) return;
    if (selected.cameras.some((c) => c.ip === assignIp)) {
      toast.info("That camera is already assigned to this room.");
      return;
    }
    const dx = Math.max(0, Math.min(assignDx, selected.w - 1));
    const dy = Math.max(0, Math.min(assignDy, selected.h - 1));
    updateSelected({ cameras: [...selected.cameras, { ip: assignIp, dx, dy }] });
    assignIp = "";
    assignDx = 0;
    assignDy = 0;
  }

  function unassignCamera(ip: string) {
    if (!selected) return;
    updateSelected({ cameras: selected.cameras.filter((c) => c.ip !== ip) });
  }

  async function save() {
    saving = true;
    try {
      await invoke("save_map", { rooms });
      dirty = false;
      toast.success("Map saved.");
    } catch (err) {
      console.error("Failed to save map", err);
      toast.error("Failed to save map", { description: String(err) });
    } finally {
      saving = false;
    }
  }

  function goLive(ip: string) {
    goto(`/live?cam=${encodeURIComponent(ip)}`);
  }
</script>

<div class="flex h-[calc(100vh-7rem)] flex-col gap-4">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Map</h1>
      <p class="text-sm text-muted-foreground">
        3D floor plan — arrange rooms, place cameras, and jump to Live.
      </p>
    </div>
    <Button onclick={save} disabled={saving || !dirty}>
      {#if saving}
        <LoaderCircle class="size-4 animate-spin" />
        Saving…
      {:else}
        <Save class="size-4" />
        Save{dirty ? " *" : ""}
      {/if}
    </Button>
  </div>

  {#if loading}
    <p class="py-10 text-center text-sm text-muted-foreground">Loading map…</p>
  {:else}
    <div class="grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-[1fr_22rem]">
      <!-- 3D canvas -->
      <Card.Root class="min-h-[24rem] overflow-hidden p-0">
        <div class="h-full w-full bg-[#0e1320]">
          <Canvas>
            <MapScene {rooms} bind:selected={selectedId} onCameraClick={goLive} />
          </Canvas>
        </div>
      </Card.Root>

      <!-- editor panel -->
      <Card.Root class="flex min-h-0 flex-col overflow-hidden">
        <Card.Header class="flex flex-row items-center justify-between space-y-0 pb-3">
          <Card.Title class="text-base">Rooms</Card.Title>
          <Button size="sm" variant="outline" onclick={addRoom}>
            <Plus class="size-4" />
            Add
          </Button>
        </Card.Header>
        <Card.Content class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
          {#if rooms.length === 0}
            <p class="text-sm text-muted-foreground">No rooms yet. Add one to start.</p>
          {:else}
            <!-- room list -->
            <div class="flex flex-col gap-1">
              {#each rooms as room (room.id)}
                <button
                  type="button"
                  class="flex items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors
                    {selectedId === room.id
                    ? 'border-amber-500/60 bg-amber-500/10'
                    : 'border-border hover:bg-muted/50'}"
                  onclick={() => (selectedId = room.id)}
                >
                  <span class="truncate">{room.name}</span>
                  {#if room.cameras.length > 0}
                    <Badge variant="secondary" class="ml-2 shrink-0">
                      <CameraIcon class="mr-1 size-3" />
                      {room.cameras.length}
                    </Badge>
                  {/if}
                </button>
              {/each}
            </div>

            {#if selected}
              <Separator />

              <!-- selected room editor -->
              <div class="space-y-3">
                <div class="space-y-1.5">
                  <Label for="room-name">Name</Label>
                  <Input
                    id="room-name"
                    value={selected.name}
                    oninput={(e) => updateSelected({ name: e.currentTarget.value })}
                  />
                </div>

                <div class="grid grid-cols-2 gap-2">
                  <div class="space-y-1.5">
                    <Label for="room-x">X</Label>
                    <Input
                      id="room-x"
                      type="number"
                      value={selected.x}
                      oninput={(e) =>
                        updateSelected({ x: clampInt(e.currentTarget.valueAsNumber, -1000) })}
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label for="room-y">Y</Label>
                    <Input
                      id="room-y"
                      type="number"
                      value={selected.y}
                      oninput={(e) =>
                        updateSelected({ y: clampInt(e.currentTarget.valueAsNumber, -1000) })}
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label for="room-w">Width</Label>
                    <Input
                      id="room-w"
                      type="number"
                      value={selected.w}
                      oninput={(e) =>
                        updateSelected({ w: clampInt(e.currentTarget.valueAsNumber, 1) })}
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label for="room-h">Height</Label>
                    <Input
                      id="room-h"
                      type="number"
                      value={selected.h}
                      oninput={(e) =>
                        updateSelected({ h: clampInt(e.currentTarget.valueAsNumber, 1) })}
                    />
                  </div>
                </div>

                <Button
                  variant="destructive"
                  size="sm"
                  class="w-full"
                  onclick={() => (deleteOpen = true)}
                >
                  <Trash2 class="size-4" />
                  Delete room
                </Button>
              </div>

              <Separator />

              <!-- cameras -->
              <div class="space-y-3">
                <Label>Cameras</Label>

                {#if selected.cameras.length === 0}
                  <p class="text-sm text-muted-foreground">No cameras assigned.</p>
                {:else}
                  <div class="flex flex-col gap-2">
                    {#each selected.cameras as cam (cam.ip)}
                      <div
                        class="flex items-center justify-between gap-2 rounded-md border border-border px-2 py-1.5"
                      >
                        <div class="min-w-0">
                          <p class="truncate text-sm">{deviceName(cam.ip)}</p>
                          <p class="truncate text-xs text-muted-foreground tabular-nums">
                            {cam.ip} · ({cam.dx}, {cam.dy})
                          </p>
                        </div>
                        <div class="flex shrink-0 gap-1">
                          <Button
                            size="icon"
                            variant="ghost"
                            class="size-7"
                            title="Open in Live"
                            onclick={() => goLive(cam.ip)}
                          >
                            <Video class="size-4" />
                          </Button>
                          <Button
                            size="icon"
                            variant="ghost"
                            class="size-7"
                            title="Unassign"
                            onclick={() => unassignCamera(cam.ip)}
                          >
                            <Trash2 class="size-4" />
                          </Button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}

                <!-- assign a saved device -->
                <div class="space-y-2 rounded-md border border-dashed border-border p-2">
                  <Label class="text-xs text-muted-foreground">Assign a camera</Label>
                  <Select.Root type="single" bind:value={assignIp}>
                    <Select.Trigger class="w-full">{assignLabel}</Select.Trigger>
                    <Select.Content>
                      {#if assignable.length === 0}
                        <div class="px-2 py-1.5 text-sm text-muted-foreground">
                          No more saved devices.
                        </div>
                      {:else}
                        {#each assignable as d (d.ip)}
                          <Select.Item value={d.ip} label={d.name}>
                            {d.name}
                            <span class="ml-2 text-xs text-muted-foreground tabular-nums"
                              >{d.ip}</span
                            >
                          </Select.Item>
                        {/each}
                      {/if}
                    </Select.Content>
                  </Select.Root>
                  <div class="grid grid-cols-2 gap-2">
                    <div class="space-y-1">
                      <Label for="cam-dx" class="text-xs">dx</Label>
                      <Input
                        id="cam-dx"
                        type="number"
                        bind:value={assignDx}
                      />
                    </div>
                    <div class="space-y-1">
                      <Label for="cam-dy" class="text-xs">dy</Label>
                      <Input
                        id="cam-dy"
                        type="number"
                        bind:value={assignDy}
                      />
                    </div>
                  </div>
                  <Button
                    size="sm"
                    class="w-full"
                    disabled={!assignIp}
                    onclick={assignCamera}
                  >
                    <Plus class="size-4" />
                    Assign
                  </Button>
                </div>
              </div>
            {/if}
          {/if}
        </Card.Content>
      </Card.Root>
    </div>
  {/if}
</div>

<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete room?</Dialog.Title>
      <Dialog.Description>
        {selected ? `"${selected.name}" and its camera assignments will be removed.` : ""}
        This is part of the unsaved working copy until you Save.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmDelete}>Delete</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
