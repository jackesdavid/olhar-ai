<script lang="ts">
  // Scene root for the Map. Renders the floor plan top-down through an orthographic
  // camera with flat 2D navigation (pan + wheel-zoom), so it reads like a blueprint
  // while still being the 3D engine — soft shadows on the low wall extrusions give
  // the gentle depth. Camera framing is owned by the shared `Viewport`.
  import { onMount } from "svelte";
  import { T, useThrelte } from "@threlte/core";
  import { interactivity } from "@threlte/extras";
  import { PCFSoftShadowMap, type OrthographicCamera } from "three";
  import type { MapRoom } from "$lib/types";
  import type { Viewport } from "$lib/map/view3d";
  import GroundGrid from "./GroundGrid.svelte";
  import RoomMesh from "./RoomMesh.svelte";

  let {
    rooms,
    selectedId = null,
    viewport,
    onSelect,
    onCameraClick,
  }: {
    rooms: MapRoom[];
    selectedId?: number | null;
    viewport: Viewport;
    onSelect?: (id: number) => void;
    onCameraClick?: (ip: string) => void;
  } = $props();

  interactivity();
  const { renderer, size, invalidate } = useThrelte();

  let cam = $state<OrthographicCamera | undefined>(undefined);

  // Keep the viewport's notion of the canvas size current.
  $effect(() => {
    viewport.width = $size.width || 1;
    viewport.height = $size.height || 1;
  });

  // Push the viewport onto the real camera whenever it (or the size) changes.
  $effect(() => {
    if (!cam) return;
    viewport.apply(cam);
    invalidate();
  });

  // Soft shadows for the gentle 3D relief.
  onMount(() => {
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = PCFSoftShadowMap;
  });

  // ---- flat 2D navigation: pan (drag) + zoom (wheel) ----------------------
  let drag: { x: number; y: number } | null = null;

  function local(e: PointerEvent | WheelEvent): { x: number; y: number } {
    const r = renderer.domElement.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  function onDown(e: PointerEvent) {
    drag = { x: e.clientX, y: e.clientY };
    renderer.domElement.setPointerCapture?.(e.pointerId);
  }
  function onMove(e: PointerEvent) {
    if (!drag) return;
    viewport.panByPixels(e.clientX - drag.x, e.clientY - drag.y);
    drag = { x: e.clientX, y: e.clientY };
  }
  function onUp(e: PointerEvent) {
    drag = null;
    renderer.domElement.releasePointerCapture?.(e.pointerId);
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const p = local(e);
    viewport.zoomAt(p.x, p.y, e.deltaY < 0 ? 1.1 : 1 / 1.1);
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

<!-- lighting: soft ambient + an angled key light casting soft shadows -->
<T.AmbientLight intensity={0.85} />
<T.DirectionalLight
  position={[40, 70, 24]}
  intensity={1.15}
  castShadow
  shadow.mapSize.width={2048}
  shadow.mapSize.height={2048}
  shadow.camera.left={-120}
  shadow.camera.right={120}
  shadow.camera.top={120}
  shadow.camera.bottom={-120}
  shadow.camera.near={1}
  shadow.camera.far={400}
/>
<!-- faint emerald rim from below the horizon to stay on-brand -->
<T.DirectionalLight position={[-30, 18, -40]} intensity={0.35} color="#34d399" />

<GroundGrid />

{#each rooms as room (room.id)}
  <RoomMesh
    {room}
    selected={room.id === selectedId}
    onSelect={() => onSelect?.(room.id)}
    {onCameraClick}
  />
{/each}
