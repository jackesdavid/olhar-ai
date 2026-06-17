<script lang="ts">
  // Name + geometry (x, y, w, h) editor for the selected room.
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import type { MapStore } from "$lib/map/store.svelte";

  let { store }: { store: MapStore } = $props();
</script>

{#if store.selectedRoom}
  <div class="space-y-3">
    <div class="space-y-1.5">
      <Label for="room-name">Name</Label>
      <Input
        id="room-name"
        value={store.selectedRoom.name}
        oninput={(e) => store.setRoomField("name", e.currentTarget.value)}
      />
    </div>

    <div class="grid grid-cols-2 gap-2">
      <div class="space-y-1.5">
        <Label for="room-x">X</Label>
        <Input
          id="room-x"
          type="number"
          value={store.selectedRoom.x}
          oninput={(e) => store.setRoomField("x", e.currentTarget.valueAsNumber)}
        />
      </div>
      <div class="space-y-1.5">
        <Label for="room-y">Y</Label>
        <Input
          id="room-y"
          type="number"
          value={store.selectedRoom.y}
          oninput={(e) => store.setRoomField("y", e.currentTarget.valueAsNumber)}
        />
      </div>
      <div class="space-y-1.5">
        <Label for="room-w">Width</Label>
        <Input
          id="room-w"
          type="number"
          value={store.selectedRoom.w}
          oninput={(e) => store.setRoomField("w", e.currentTarget.valueAsNumber)}
        />
      </div>
      <div class="space-y-1.5">
        <Label for="room-h">Height</Label>
        <Input
          id="room-h"
          type="number"
          value={store.selectedRoom.h}
          oninput={(e) => store.setRoomField("h", e.currentTarget.valueAsNumber)}
        />
      </div>
    </div>
  </div>
{/if}
