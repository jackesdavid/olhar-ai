<script lang="ts">
  // Properties tab content: follows the current selection (marker → wall → room props),
  // plus the marker picker / drawing hints while a draw tool is active. Pure composition
  // over the shared MapStore. The room list lives in the Explorer tab now.
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Spline from "@lucide/svelte/icons/spline";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Separator } from "$lib/components/ui/separator";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { ToolState } from "$lib/map/tools.svelte";
  import RoomForm from "./RoomForm.svelte";
  import CameraAssign from "./CameraAssign.svelte";
  import WallProps from "./WallProps.svelte";
  import MarkerProps from "./MarkerProps.svelte";
  import MarkerPicker from "./MarkerPicker.svelte";

  let {
    store,
    tools,
    onGoLive,
  }: { store: MapStore; tools: ToolState; onGoLive: (ip: string) => void } = $props();

  let deleteOpen = $state(false);

  function confirmDelete() {
    if (store.selectedRoom) store.deleteRoom(store.selectedRoom.id);
    deleteOpen = false;
  }
</script>

<div class="flex flex-col gap-4">
  {#if tools.mode === "wall"}
    <div class="flex items-start gap-2 rounded-md border border-primary/30 bg-primary/5 p-2 text-xs text-muted-foreground">
      <Spline class="mt-0.5 size-4 shrink-0 text-primary" />
      <span>Click to drop points and chain a wall. Hold <kbd>Shift</kbd> to snap to 45°/90° (diagonals). Drag a thin wall thicker to make it solid.</span>
    </div>
  {/if}

  {#if tools.mode === "marker"}
    <div class="space-y-2">
      <p class="text-xs text-muted-foreground">Pick an item, then click the floor to place it.</p>
      <MarkerPicker {tools} />
    </div>
    <Separator />
  {/if}

  {#if store.selectedMarker}
    <MarkerProps {store} />
  {:else if store.selectedWall}
    <WallProps {store} />
  {:else if store.selectedRoom}
    <RoomForm {store} />
    <Button variant="destructive" size="sm" class="w-full" onclick={() => (deleteOpen = true)}>
      <Trash2 class="size-4" />
      Delete room
    </Button>
    <Separator />
    <CameraAssign {store} {onGoLive} />
  {:else if tools.mode === "select"}
    <div class="flex flex-col items-center gap-2 rounded-md border border-dashed border-border px-4 py-8 text-center text-xs text-muted-foreground">
      <MousePointer2 class="size-5 opacity-60" />
      <span>Select an item on the plan (or in the Explorer) to edit its properties.</span>
    </div>
  {/if}
</div>

<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete room?</Dialog.Title>
      <Dialog.Description>
        {store.selectedRoom
          ? `"${store.selectedRoom.name}" and its camera assignments will be removed.`
          : ""}
        This is part of the unsaved working copy until you Save.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmDelete}>Delete</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
