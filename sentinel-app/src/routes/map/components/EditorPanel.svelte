<script lang="ts">
  // Right-hand editor column: room list, the selected room's form, its cameras,
  // and add/delete. Pure composition over the shared MapStore.
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Separator } from "$lib/components/ui/separator";
  import type { MapStore } from "$lib/map/store.svelte";
  import RoomList from "./RoomList.svelte";
  import RoomForm from "./RoomForm.svelte";
  import CameraAssign from "./CameraAssign.svelte";

  let { store, onGoLive }: { store: MapStore; onGoLive: (ip: string) => void } = $props();

  let deleteOpen = $state(false);

  function confirmDelete() {
    if (store.selected) store.deleteRoom(store.selected.id);
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
    {#if store.rooms.length === 0}
      <p class="text-sm text-muted-foreground">No rooms yet. Add one to start.</p>
    {:else}
      <RoomList {store} />

      {#if store.selected}
        <Separator />
        <RoomForm {store} />
        <Button variant="destructive" size="sm" class="w-full" onclick={() => (deleteOpen = true)}>
          <Trash2 class="size-4" />
          Delete room
        </Button>
        <Separator />
        <CameraAssign {store} {onGoLive} />
      {/if}
    {/if}
  </Card.Content>
</Card.Root>

<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete room?</Dialog.Title>
      <Dialog.Description>
        {store.selected
          ? `"${store.selected.name}" and its camera assignments will be removed.`
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
