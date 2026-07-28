<script lang="ts">
  // Cameras assigned to the selected room: list (with FOV rotation + open-in-Live +
  // unassign) and a picker to assign another saved device.
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Video from "@lucide/svelte/icons/video";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import type { MapStore } from "$lib/map/store.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].map);

  let { store, onGoLive }: { store: MapStore; onGoLive: (ip: string) => void } = $props();

  let assignIp = $state("");
  let assignDx = $state(0);
  let assignDy = $state(0);

  const assignLabel = $derived(
    assignIp ? store.deviceName(assignIp) : t.selectCameraToAssign,
  );

  function doAssign() {
    if (store.assignCamera(assignIp, assignDx, assignDy)) {
      assignIp = "";
      assignDx = 0;
      assignDy = 0;
    }
  }
</script>

{#if store.selectedRoom}
  <div class="space-y-3">
    <Label>{t.cameras}</Label>

    {#if store.selectedRoom.cameras.length === 0}
      <p class="text-sm text-muted-foreground">{t.noCamerasAssigned}</p>
    {:else}
      <div class="flex flex-col gap-2">
        {#each store.selectedRoom.cameras as cam (cam.ip)}
          <div class="space-y-2 rounded-md border border-border px-2 py-1.5">
            <div class="flex items-center justify-between gap-2">
              <div class="min-w-0">
                <p class="truncate text-sm">{store.deviceName(cam.ip)}</p>
                <p class="truncate text-xs text-muted-foreground tabular-nums">
                  {cam.ip} · ({cam.dx}, {cam.dy})
                </p>
              </div>
              <div class="flex shrink-0 gap-1">
                <Button
                  size="icon"
                  variant="ghost"
                  class="size-7"
                  title={t.openInLive}
                  onclick={() => onGoLive(cam.ip)}
                >
                  <Video class="size-4" />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  class="size-7"
                  title={t.unassign}
                  onclick={() => store.unassignCamera(cam.ip)}
                >
                  <Trash2 class="size-4" />
                </Button>
              </div>
            </div>
            <div class="flex items-center gap-2">
              <Label class="w-16 shrink-0 text-xs text-muted-foreground">{t.facing}</Label>
              <input
                type="range"
                min="0"
                max="359"
                value={cam.rot ?? 0}
                class="h-1.5 w-full accent-primary"
                oninput={(e) => store.setCameraRotation(cam.ip, e.currentTarget.valueAsNumber)}
              />
              <span class="w-10 text-right text-xs tabular-nums text-muted-foreground">
                {cam.rot ?? 0}°
              </span>
            </div>
            <div class="grid grid-cols-2 gap-2">
              <div class="space-y-1">
                <Label class="text-xs text-muted-foreground">dx</Label>
                <Input
                  type="number"
                  class="h-8"
                  value={cam.dx}
                  oninput={(e) => store.setCameraPosition(cam.ip, e.currentTarget.valueAsNumber, cam.dy)}
                />
              </div>
              <div class="space-y-1">
                <Label class="text-xs text-muted-foreground">dy</Label>
                <Input
                  type="number"
                  class="h-8"
                  value={cam.dy}
                  oninput={(e) => store.setCameraPosition(cam.ip, cam.dx, e.currentTarget.valueAsNumber)}
                />
              </div>
            </div>
            <div class="flex items-center gap-2">
              <Label class="w-16 shrink-0 text-xs text-muted-foreground">FOV</Label>
              <input
                type="range"
                min="20"
                max="170"
                value={cam.fov ?? 62}
                class="h-1.5 w-full accent-primary"
                oninput={(e) => store.setCameraFov(cam.ip, e.currentTarget.valueAsNumber)}
              />
              <span class="w-10 text-right text-xs tabular-nums text-muted-foreground">{Math.round(cam.fov ?? 62)}°</span>
            </div>
            <div class="flex items-center gap-2">
              <Label class="w-16 shrink-0 text-xs text-muted-foreground">{t.length}</Label>
              <input
                type="range"
                min="1"
                max="20"
                step="0.5"
                value={cam.range ?? 4.2}
                class="h-1.5 w-full accent-primary"
                oninput={(e) => store.setCameraRange(cam.ip, e.currentTarget.valueAsNumber)}
              />
              <span class="w-10 text-right text-xs tabular-nums text-muted-foreground">{(cam.range ?? 4.2).toFixed(1)}</span>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- assign a saved device -->
    <div class="space-y-2 rounded-md border border-dashed border-border p-2">
      <Label class="text-xs text-muted-foreground">{t.assignACamera}</Label>
      <Select.Root type="single" bind:value={assignIp}>
        <Select.Trigger class="w-full">{assignLabel}</Select.Trigger>
        <Select.Content>
          {#if store.assignable.length === 0}
            <div class="px-2 py-1.5 text-sm text-muted-foreground">{t.noMoreDevices}</div>
          {:else}
            {#each store.assignable as d (d.ip)}
              <Select.Item value={d.ip} label={d.name}>
                {d.name}
                <span class="ml-2 text-xs text-muted-foreground tabular-nums">{d.ip}</span>
              </Select.Item>
            {/each}
          {/if}
        </Select.Content>
      </Select.Root>
      <div class="grid grid-cols-2 gap-2">
        <div class="space-y-1">
          <Label for="cam-dx" class="text-xs">dx</Label>
          <Input id="cam-dx" type="number" bind:value={assignDx} />
        </div>
        <div class="space-y-1">
          <Label for="cam-dy" class="text-xs">dy</Label>
          <Input id="cam-dy" type="number" bind:value={assignDy} />
        </div>
      </div>
      <Button size="sm" class="w-full" disabled={!assignIp} onclick={doAssign}>
        <Plus class="size-4" />
        Assign
      </Button>
    </div>
  </div>
{/if}
