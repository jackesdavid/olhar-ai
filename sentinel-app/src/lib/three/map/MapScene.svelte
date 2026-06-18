<script lang="ts">
  // Scene root for the Map. Top-down orthographic view with flat 2D navigation, and
  // tool-aware pointer handling: Select (pick / drag entities, pan empty space) and
  // Wall (click-chain segments with grid + vertex + ortho snapping). All picking and
  // screen→world go through the shared Viewport, so everything shares one coordinate
  // model. Soft shadows on the wall extrusions give the gentle 3D depth.
  import { onMount } from "svelte";
  import { T, useThrelte } from "@threlte/core";
  import { interactivity } from "@threlte/extras";
  import { PCFSoftShadowMap, type OrthographicCamera } from "three";
  import type { MapStore } from "$lib/map/store.svelte";
  import type { PresenceStore } from "$lib/map/presence.svelte";
  import type { Viewport } from "$lib/map/view3d.svelte";
  import { resolvePoint, resolveMarkerPoint, wallVertices, type Pt, type ToolState } from "$lib/map/tools.svelte";
  import { hitTest, nearWallEndpoint } from "$lib/map/pick";
  import { categoryColor } from "$lib/map/categories";
  import GroundGrid from "./GroundGrid.svelte";
  import RoomMesh from "./RoomMesh.svelte";
  import WallLayer from "./WallLayer.svelte";
  import MarkerLayer from "./MarkerLayer.svelte";
  import CameraLayer from "./CameraLayer.svelte";
  import DrawPreview from "./DrawPreview.svelte";
  import PresencePulse from "./PresencePulse.svelte";

  let {
    store,
    viewport,
    tools,
    presence,
    onCameraClick,
  }: {
    store: MapStore;
    viewport: Viewport;
    tools: ToolState;
    presence?: PresenceStore;
    onCameraClick?: (ip: string) => void;
  } = $props();

  interactivity();
  const { renderer, size, invalidate } = useThrelte();

  let cam = $state<OrthographicCamera | undefined>(undefined);

  // One effect drives the camera: reading $size directly guarantees a re-apply on every
  // canvas resize (e.g. collapsing the side panel), and apply() reads zoom/pan so it also
  // re-runs on manual zoom/pan. (Splitting size and apply into two effects let the resize
  // case slip through — the view reset until the next manual zoom.)
  $effect(() => {
    viewport.width = $size.width || 1;
    viewport.height = $size.height || 1;
    if (cam) {
      viewport.apply(cam);
      invalidate();
    }
  });

  onMount(() => {
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = PCFSoftShadowMap;
  });

  // ---- pointer → world ----------------------------------------------------
  function worldAt(e: PointerEvent): Pt {
    const r = renderer.domElement.getBoundingClientRect();
    return viewport.screenToWorld(e.clientX - r.left, e.clientY - r.top);
  }
  function snap(raw: Pt, from: Pt | null, ortho: boolean): Pt {
    return resolvePoint(raw, { from, ortho, verts: wallVertices(store.walls) });
  }

  type Drag =
    | { mode: "pan"; sx: number; sy: number }
    | { mode: "endpoint"; id: number; which: 1 | 2; started: boolean }
    | { mode: "room" | "wall" | "marker"; id: number; last: Pt; started: boolean };
  let drag: Drag | null = null;

  function onDown(e: PointerEvent) {
    renderer.domElement.setPointerCapture?.(e.pointerId);

    if (tools.mode === "wall") {
      const sp = snap(worldAt(e), tools.draft, e.shiftKey);
      if (!tools.draft) tools.draft = sp;
      else {
        store.addWall(tools.draft.x, tools.draft.z, sp.x, sp.z);
        tools.draft = sp; // chain
      }
      return;
    }
    if (tools.mode === "marker") {
      const sp = resolveMarkerPoint(worldAt(e), store.walls);
      store.addMarker(tools.markerIcon, sp.x, sp.z);
      return;
    }

    // select mode
    const raw = worldAt(e);
    const ep = nearWallEndpoint(raw, store.walls);
    if (ep) {
      store.selectWall(ep.id);
      drag = { mode: "endpoint", id: ep.id, which: ep.which, started: false };
      return;
    }
    const hit = hitTest(raw, store.rooms, store.walls, store.markers);
    if (hit) {
      if (hit.kind === "room") store.selectRoom(hit.id);
      else if (hit.kind === "wall") store.selectWall(hit.id);
      else store.selectMarker(hit.id);
      drag = { mode: hit.kind, id: hit.id, last: snap(raw, null, false), started: false };
      return;
    }
    store.clearSelection();
    drag = { mode: "pan", sx: e.clientX, sy: e.clientY };
  }

  function onMove(e: PointerEvent) {
    // live snapped cursor for the draw preview
    if (tools.mode === "wall") tools.hover = snap(worldAt(e), tools.draft, e.shiftKey);
    else if (tools.mode === "marker") tools.hover = resolveMarkerPoint(worldAt(e), store.walls);
    else tools.hover = null;

    if (!drag) return;

    if (drag.mode === "pan") {
      viewport.panByPixels(e.clientX - drag.sx, e.clientY - drag.sy);
      drag.sx = e.clientX;
      drag.sy = e.clientY;
      return;
    }

    const sp = snap(worldAt(e), null, e.shiftKey);

    if (drag.mode === "endpoint") {
      if (!drag.started) {
        store.checkpoint();
        drag.started = true;
      }
      if (drag.which === 1) store.updateWall(drag.id, { x1: sp.x, y1: sp.z });
      else store.updateWall(drag.id, { x2: sp.x, y2: sp.z });
      return;
    }

    // Markers snap absolutely (onto a wall when near one, else grid) so they stay aligned.
    if (drag.mode === "marker") {
      const mid = drag.id;
      const mp = resolveMarkerPoint(worldAt(e), store.walls);
      const m = store.markers.find((x) => x.id === mid);
      if (m && (m.x !== mp.x || m.y !== mp.z)) {
        if (!drag.started) {
          store.checkpoint();
          drag.started = true;
        }
        store.updateMarker(m.id, { x: mp.x, y: mp.z });
      }
      return;
    }

    const ddx = sp.x - drag.last.x;
    const ddz = sp.z - drag.last.z;
    if (ddx === 0 && ddz === 0) return;
    if (!drag.started) {
      store.checkpoint();
      drag.started = true;
    }
    const id = drag.id;
    if (drag.mode === "room") {
      const r = store.rooms.find((x) => x.id === id);
      if (r) store.updateRoom({ x: r.x + ddx, y: r.y + ddz });
    } else if (drag.mode === "wall") {
      const w = store.walls.find((x) => x.id === id);
      if (w) store.updateWall(w.id, { x1: w.x1 + ddx, y1: w.y1 + ddz, x2: w.x2 + ddx, y2: w.y2 + ddz });
    }
    drag.last = sp;
  }

  function onUp(e: PointerEvent) {
    drag = null;
    renderer.domElement.releasePointerCapture?.(e.pointerId);
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const r = renderer.domElement.getBoundingClientRect();
    viewport.zoomAt(e.clientX - r.left, e.clientY - r.top, e.deltaY < 0 ? 1.1 : 1 / 1.1);
  }

  onMount(() => {
    const el = renderer.domElement;
    el.addEventListener("pointerdown", onDown);
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointerleave", onUp);
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      el.removeEventListener("pointerdown", onDown);
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointerleave", onUp);
      el.removeEventListener("wheel", onWheel);
    };
  });
</script>

<T.OrthographicCamera makeDefault bind:ref={cam} position={[0, 200, 0]} />

<T.AmbientLight intensity={0.5} />
<T.DirectionalLight
  position={[90, 38, 20]}
  intensity={1.7}
  castShadow
  shadow.mapSize.width={2048}
  shadow.mapSize.height={2048}
  shadow.radius={4}
  shadow.camera.left={-80}
  shadow.camera.right={80}
  shadow.camera.top={80}
  shadow.camera.bottom={-80}
  shadow.camera.near={1}
  shadow.camera.far={400}
/>
<!-- cool neutral fill — keeps the plan reading as paper-white, not green-tinted -->
<T.DirectionalLight position={[-30, 18, -40]} intensity={0.3} color="#aab9c7" />

<GroundGrid />

{#each store.rooms as room (room.id)}
  <RoomMesh {room} selected={room.id === store.selectedRoomId} />
{/each}

<WallLayer {store} />
<MarkerLayer {store} />
<CameraLayer {store} {onCameraClick} />

{#if presence}
  {#each presence.pulses as p (p.id)}
    <PresencePulse x={p.x} z={p.z} color={categoryColor(p.category)} />
  {/each}
{/if}

{#if tools.mode === "wall"}
  <DrawPreview from={tools.draft} to={tools.hover} />
{:else if tools.mode === "marker"}
  <DrawPreview to={tools.hover} />
{/if}
