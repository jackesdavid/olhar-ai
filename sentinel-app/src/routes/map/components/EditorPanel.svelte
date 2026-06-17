<script lang="ts">
  // Right-hand editor column. Pure composition over the shared MapStore: the room
  // list, then a properties section that follows the selection (a wall's props when
  // a wall is selected, otherwise the room form + cameras).
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Spline from "@lucide/svelte/icons/spline";
  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Separator } from "$lib/components/ui/separator";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { ToolState } from "$lib/map/tools";
  import RoomList from "./RoomList.svelte";
  import RoomForm from "./RoomForm.svelte";
  import CameraAssign from "./CameraAssign.svelte";
  import WallProps from "./WallProps.svelte";

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

<Card.Root class="flex min-h-0 flex-col overflow-hidden">
  <Card.Header class="flex flex-row items-center justify-between space-y-0 pb-3">
    <Card.Title class="text-base">Rooms</Card.Title>
    <Button size="sm" variant="outline" onclick={() => store.addRoom()}>
      <Plus class="size-4" />
      Add
    </Button>
  </Card.Header>
  <Card.Content class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
    {#if tools.mode === "wall"}
      <div class="flex items-start gap-2 rounded-md border border-primary/30 bg-primary/5 p-2 text-xs text-muted-foreground">
        <Spline class="mt-0.5 size-4 shrink-0 text-primary" />
        <span>Click to drop points and chain a wall. Hold <kbd>Shift</kbd> to lock to 90°. Drag a thin wall thicker to make it solid.</span>
      </div>
    {/if}

    {#if store.rooms.length === 0}
      <p class="text-sm text-muted-foreground">No rooms yet. Add a zone, or draw walls.</p>
    {:else}
      <RoomList {store} />
    {/if}

    {#if store.selectedWall}
      <Separator />
      <WallProps {store} />
    {:else if store.selectedRoom}
      <Separator />
      <RoomForm {store} />
      <Button variant="destructive" size="sm" class="w-full" onclick={() => (deleteOpen = true)}>
        <Trash2 class="size-4" />
        Delete room
      </Button>
      <Separator />
      <CameraAssign {store} {onGoLive} />
    {/if}
  </Card.Content>
</Card.Root>

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
