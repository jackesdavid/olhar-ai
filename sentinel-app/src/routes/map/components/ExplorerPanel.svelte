<script lang="ts">
  // Explorer tab: a flat tree of everything on the plan — rooms (with their cameras),
  // walls, and markers. Click a row to select it (the Properties tab follows the
  // selection); the trash button removes it. Pure composition over the MapStore.
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Square from "@lucide/svelte/icons/square";
  import Spline from "@lucide/svelte/icons/spline";
  import Armchair from "@lucide/svelte/icons/armchair";
  import Cctv from "@lucide/svelte/icons/cctv";
  import { Button } from "$lib/components/ui/button";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { MapWall } from "$lib/types";
  import { iconLabel } from "$lib/map/icons";

  let { store }: { store: MapStore } = $props();

  function wallLen(w: MapWall): number {
    return Math.round(Math.hypot(w.x2 - w.x1, w.y2 - w.y1) * 10) / 10;
  }
  function rowClass(active: boolean): string {
    return active
      ? "border-primary/40 bg-primary/10 text-foreground"
      : "border-transparent text-muted-foreground hover:bg-hover hover:text-foreground";
  }
  function markerName(icon: string, label: string | null | undefined): string {
    return label?.trim() ? label : iconLabel(icon);
  }
</script>

<div class="space-y-5">
  <!-- Rooms -->
  <section class="space-y-1.5">
    <header class="flex items-center justify-between">
      <h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        Rooms <span class="tabular-nums">({store.rooms.length})</span>
      </h3>
      <Button size="sm" variant="ghost" class="h-7 px-2 text-xs" onclick={() => store.addRoom()}>
        <Plus class="size-3.5" /> Add
      </Button>
    </header>
    {#if store.rooms.length === 0}
      <p class="px-1 text-xs text-muted-foreground">No rooms yet.</p>
    {:else}
      <ul class="space-y-0.5">
        {#each store.rooms as room (room.id)}
          <li>
            <div class="group flex items-center gap-1 rounded-md border px-2 py-1.5 text-sm {rowClass(room.id === store.selectedRoomId)}">
              <button type="button" class="interactive flex min-w-0 flex-1 items-center gap-2" onclick={() => store.selectRoom(room.id)}>
                <Square class="size-3.5 shrink-0 opacity-70" />
                <span class="truncate">{room.name}</span>
                {#if room.cameras.length > 0}
                  <span class="ml-auto inline-flex items-center gap-1 text-[10px] text-muted-foreground"><Cctv class="size-3" />{room.cameras.length}</span>
                {/if}
              </button>
              <button type="button" class="interactive rounded p-1 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100 [@media(hover:none)]:opacity-100" title="Delete room" onclick={() => store.deleteRoom(room.id)}>
                <Trash2 class="size-3.5" />
              </button>
            </div>
            {#if room.cameras.length > 0}
              <ul class="ml-5 mt-0.5 space-y-0.5 border-l border-border/60 pl-2">
                {#each room.cameras as cam (cam.ip)}
                  <li class="group flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground">
                    <Cctv class="size-3 shrink-0 opacity-70" />
                    <span class="truncate">{store.deviceName(cam.ip)}</span>
                    <button type="button" class="interactive ml-auto rounded p-0.5 opacity-0 hover:text-destructive group-hover:opacity-100 [@media(hover:none)]:opacity-100" title="Unassign camera" onclick={() => { store.selectRoom(room.id); store.unassignCamera(cam.ip); }}>
                      <Trash2 class="size-3" />
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- Walls -->
  <section class="space-y-1.5">
    <h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
      Walls <span class="tabular-nums">({store.walls.length})</span>
    </h3>
    {#if store.walls.length === 0}
      <p class="px-1 text-xs text-muted-foreground">No walls drawn.</p>
    {:else}
      <ul class="space-y-0.5">
        {#each store.walls as wall, i (wall.id)}
          <li class="group flex items-center gap-1 rounded-md border px-2 py-1.5 text-sm {rowClass(wall.id === store.selectedWallId)}">
            <button type="button" class="interactive flex min-w-0 flex-1 items-center gap-2" onclick={() => store.selectWall(wall.id)}>
              <Spline class="size-3.5 shrink-0 opacity-70" />
              <span class="truncate">Wall {i + 1}</span>
              <span class="ml-auto text-[10px] tabular-nums text-muted-foreground">{wallLen(wall)}m</span>
            </button>
            <button type="button" class="interactive rounded p-1 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100 [@media(hover:none)]:opacity-100" title="Delete wall" onclick={() => store.deleteWall(wall.id)}>
              <Trash2 class="size-3.5" />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- Markers -->
  <section class="space-y-1.5">
    <h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
      Markers <span class="tabular-nums">({store.markers.length})</span>
    </h3>
    {#if store.markers.length === 0}
      <p class="px-1 text-xs text-muted-foreground">No markers placed.</p>
    {:else}
      <ul class="space-y-0.5">
        {#each store.markers as marker (marker.id)}
          <li class="group flex items-center gap-1 rounded-md border px-2 py-1.5 text-sm {rowClass(marker.id === store.selectedMarkerId)}">
            <button type="button" class="interactive flex min-w-0 flex-1 items-center gap-2" onclick={() => store.selectMarker(marker.id)}>
              <Armchair class="size-3.5 shrink-0 opacity-70" />
              <span class="truncate">{markerName(marker.icon, marker.label)}</span>
            </button>
            <button type="button" class="interactive rounded p-1 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100 [@media(hover:none)]:opacity-100" title="Delete marker" onclick={() => store.deleteMarker(marker.id)}>
              <Trash2 class="size-3.5" />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>
