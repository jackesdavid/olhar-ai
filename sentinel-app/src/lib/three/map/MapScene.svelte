<script lang="ts">
  // Scene root for the Map. Top-down orthographic view with flat 2D navigation, and
  // tool-aware pointer handling. All picking/screen→world go through the shared
  // Viewport, so everything shares one coordinate model.
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
  import MapTextLayer from "./MapTextLayer.svelte";
  import ShapeLayer from "./ShapeLayer.svelte";
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
    | { mode: "room" | "wall" | "marker" | "text" | "shape"; id: number; last: Pt; started: boolean };
  let drag: Drag | null = null;

  function onDown(e: PointerEvent) {
    renderer.domElement.setPointerCapture?.(e.pointerId);
    const m = tools.mode;

    // ---- wall / line tool (two-click chain) --------------------------------
    if (m === "wall" || m === "line") {
      const sp = snap(worldAt(e), tools.draft, e.shiftKey);
      const style = m === "line" ? "line" : "solid";
      if (!tools.draft) {
        tools.draft = sp;
      } else {
        store.addWall(tools.draft.x, tools.draft.z, sp.x, sp.z, 0.12, style);
        tools.draft = sp; // chain
      }
      return;
    }

    // ---- marker tool -------------------------------------------------------
    if (m === "marker") {
      const sp = resolveMarkerPoint(worldAt(e), store.walls);
      store.addMarker(tools.markerIcon, sp.x, sp.z);
      return;
    }

    // ---- text tool (single click) ------------------------------------------
    if (m === "text") {
      const sp = snap(worldAt(e), null, false);
      store.addText(sp.x, sp.z);
      return;
    }

    // ---- rect tool (two clicks: corner A → corner B) -----------------------
    if (m === "rect") {
      const sp = snap(worldAt(e), null, false);
      if (!tools.draft) {
        tools.draft = sp;
      } else {
        const x1 = tools.draft.x, z1 = tools.draft.z;
        const x2 = sp.x, z2 = sp.z;
        const w = Math.abs(x2 - x1);
        const h = Math.abs(z2 - z1);
        if (w > 0.05 || h > 0.05) {
          store.addShape({
            kind: "rect",
            x: (x1 + x2) / 2,
            y: (z1 + z2) / 2,
            w,
            h,
            color: "#ffffff",
          });
        }
        tools.draft = null;
      }
      return;
    }

    // ---- circle tool (two clicks: center → edge) ---------------------------
    if (m === "circle") {
      const sp = snap(worldAt(e), null, false);
      if (!tools.draft) {
        tools.draft = sp;
      } else {
        const r = Math.hypot(sp.x - tools.draft.x, sp.z - tools.draft.z);
        if (r > 0.1) {
          store.addShape({
            kind: "circle",
            x: tools.draft.x,
            y: tools.draft.z,
            r,
            color: "#ffffff",
          });
        }
        tools.draft = null;
      }
      return;
    }

    // ---- triangle tool (three clicks) --------------------------------------
    if (m === "triangle") {
      const sp = snap(worldAt(e), null, false);
      if (!tools.draft) {
        tools.draft = sp;
      } else if (!tools.draftB) {
        tools.draftB = sp;
      } else {
        const pts: [number, number][] = [
          [tools.draft.x, tools.draft.z],
          [tools.draftB.x, tools.draftB.z],
          [sp.x, sp.z],
        ];
        store.addShape({ kind: "triangle", x: 0, y: 0, pts, color: "#ffffff" });
        tools.draft = null;
        tools.draftB = null;
      }
      return;
    }

    // ---- select mode -------------------------------------------------------
    const raw = worldAt(e);
    const ep = nearWallEndpoint(raw, store.walls);
    if (ep) {
      store.selectWall(ep.id);
      drag = { mode: "endpoint", id: ep.id, which: ep.which, started: false };
      return;
    }
    const hit = hitTest(raw, store.rooms, store.walls, store.markers, store.texts, store.shapes);
    if (hit) {
      if (hit.kind === "room") store.selectRoom(hit.id);
      else if (hit.kind === "wall") store.selectWall(hit.id);
      else if (hit.kind === "marker") store.selectMarker(hit.id);
      else if (hit.kind === "text") store.selectText(hit.id);
      else if (hit.kind === "shape") store.selectShape(hit.id);
      drag = { mode: hit.kind, id: hit.id, last: snap(raw, null, false), started: false };
      return;
    }
    store.clearSelection();
    drag = { mode: "pan", sx: e.clientX, sy: e.clientY };
  }

  function onMove(e: PointerEvent) {
    const m = tools.mode;
    if (m === "wall" || m === "line") tools.hover = snap(worldAt(e), tools.draft, e.shiftKey);
    else if (m === "marker") tools.hover = resolveMarkerPoint(worldAt(e), store.walls);
    else if (m === "text" || m === "rect" || m === "circle" || m === "triangle") {
      tools.hover = snap(worldAt(e), tools.draft, false);
    } else tools.hover = null;

    if (!drag) return;

    if (drag.mode === "pan") {
      viewport.panByPixels(e.clientX - drag.sx, e.clientY - drag.sy);
      drag.sx = e.clientX;
      drag.sy = e.clientY;
      return;
    }

    const sp = snap(worldAt(e), null, e.shiftKey);

    if (drag.mode === "endpoint") {
      if (!drag.started) { store.checkpoint(); drag.started = true; }
      if (drag.which === 1) store.updateWall(drag.id, { x1: sp.x, y1: sp.z });
      else store.updateWall(drag.id, { x2: sp.x, y2: sp.z });
      return;
    }

    if (drag.mode === "marker") {
      const mp = resolveMarkerPoint(worldAt(e), store.walls);
      const m2 = store.markers.find((x) => x.id === drag!.id);
      if (m2 && (m2.x !== mp.x || m2.y !== mp.z)) {
        if (!drag.started) { store.checkpoint(); drag.started = true; }
        store.updateMarker(m2.id, { x: mp.x, y: mp.z });
      }
      return;
    }

    // Narrow: pan/endpoint/marker already returned; only room|wall|text|shape remain.
    if (!("last" in drag)) return;
    const id = drag.id;
    const ddx = sp.x - drag.last.x;
    const ddz = sp.z - drag.last.z;
    if (ddx === 0 && ddz === 0) return;
    if (!drag.started) { store.checkpoint(); drag.started = true; }
    if (drag.mode === "room") {
      const r = store.rooms.find((x) => x.id === id);
      if (r) store.updateRoom({ x: r.x + ddx, y: r.y + ddz });
    } else if (drag.mode === "wall") {
      const w = store.walls.find((x) => x.id === id);
      if (w) store.updateWall(w.id, { x1: w.x1 + ddx, y1: w.y1 + ddz, x2: w.x2 + ddx, y2: w.y2 + ddz });
    } else if (drag.mode === "text") {
      const t = store.texts.find((x) => x.id === id);
      if (t) store.updateText(t.id, { x: t.x + ddx, y: t.y + ddz });
    } else if (drag.mode === "shape") {
      const s = store.shapes.find((x) => x.id === id);
      if (s) {
        if (s.kind === "triangle" && s.pts) {
          store.updateShape(s.id, {
            pts: s.pts.map(([px, py]) => [px + ddx, py + ddz]) as [number, number][],
          });
        } else {
          store.updateShape(s.id, { x: s.x + ddx, y: s.y + ddz });
        }
      }
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

<T.AmbientLight intensity={1.4} />
<T.DirectionalLight
  position={[90, 38, 20]}
  intensity={2.4}
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
<T.DirectionalLight position={[-30, 18, -40]} intensity={0.8} color="#aab9c7" />

<GroundGrid />

{#each store.rooms as room (room.id)}
  <RoomMesh {room} selected={room.id === store.selectedRoomId} />
{/each}

<WallLayer {store} />
<MarkerLayer {store} />
<ShapeLayer {store} />
<MapTextLayer {store} />
<CameraLayer {store} {onCameraClick} />

{#if presence}
  {#each presence.pulses as p (p.id)}
    <PresencePulse x={p.x} z={p.z} color={categoryColor(p.category)} />
  {/each}
{/if}

{#if tools.mode !== "select"}
  <DrawPreview
    mode={tools.mode}
    from={tools.draft}
    to={tools.hover}
    draftB={tools.draftB}
  />
{/if}
