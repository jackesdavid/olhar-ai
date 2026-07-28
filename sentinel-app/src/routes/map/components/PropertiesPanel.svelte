<script lang="ts">
  // Properties tab content: follows the current selection (text/shape/marker/wall/room props),
  // plus the marker picker / drawing hints while a draw tool is active.
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
  import TextProps from "./TextProps.svelte";
  import ShapeProps from "./ShapeProps.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].map);
  const c = $derived(dict[lang()].common);

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

  const DRAW_HINTS: Partial<Record<string, string>> = $derived({
    wall: t.hintWall,
    line: t.hintLine,
    rect: t.hintRect,
    triangle: t.hintTriangle,
    circle: t.hintCircle,
    text: t.hintText,
  });
</script>

<div class="flex flex-col gap-4">
  {#if tools.mode !== "select" && tools.mode !== "marker"}
    {@const hint = DRAW_HINTS[tools.mode]}
    {#if hint}
      <div class="flex items-start gap-2 rounded-md border border-primary/30 bg-primary/5 p-2 text-xs text-muted-foreground">
        <Spline class="mt-0.5 size-4 shrink-0 text-primary" />
        <span>{hint}</span>
      </div>
    {/if}
  {/if}

  {#if tools.mode === "marker"}
    <div class="space-y-2">
      <p class="text-xs text-muted-foreground">{t.pickItemHint}</p>
      <MarkerPicker {tools} />
    </div>
    <Separator />
  {/if}

  {#if store.selectedText}
    <TextProps {store} />
  {:else if store.selectedShape}
    <ShapeProps {store} />
  {:else if store.selectedMarker}
    <MarkerProps {store} />
  {:else if store.selectedWall}
    <WallProps {store} />
  {:else if store.selectedRoom}
    <RoomForm {store} />
    <Button variant="destructive" size="sm" class="w-full" onclick={() => (deleteOpen = true)}>
      <Trash2 class="size-4" />
      {t.deleteRoom}
    </Button>
    <Separator />
    <CameraAssign {store} {onGoLive} />
  {:else if tools.mode === "select"}
    <div class="flex flex-col items-center gap-2 rounded-md border border-dashed border-border px-4 py-8 text-center text-xs text-muted-foreground">
      <MousePointer2 class="size-5 opacity-60" />
      <span>{t.selectPrompt}</span>
    </div>
  {/if}
</div>

<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{t.deleteRoomTitle}</Dialog.Title>
      <Dialog.Description>
        {store.selectedRoom ? t.deleteRoomNamed(store.selectedRoom.name) : ""}
        {t.unsavedNote}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)}>{c.cancel}</Button>
      <Button variant="destructive" onclick={confirmDelete}>{t.delete}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
