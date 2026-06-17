// Ortho top-down viewport for the Map scene. The floor plan lives on the world
// XZ plane (grid x → world x, grid y → world z) and we look straight down -Y, so
// the whole thing reads as a flat 2D blueprint while still being the 3D engine.
//
// Navigation is 2D only: pan moves the camera centre in the XZ plane, zoom scales
// the orthographic frustum. This module owns the math (screen↔world, zoom-at-cursor,
// fit-to-bounds); MapScene applies it to a real OrthographicCamera each frame.
import type { OrthographicCamera } from "three";

/** Vertical world extent visible at zoom = 1 (grid units). */
const BASE_VIEW = 28;
const MIN_ZOOM = 0.25;
const MAX_ZOOM = 6;

/** Camera height above the floor — large enough that near/far always contain the
 * (flat) scene; irrelevant to apparent size because the camera is orthographic. */
const CAM_Y = 200;

export type Bounds = { minX: number; maxX: number; minZ: number; maxZ: number };

function clampZoom(z: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));
}

/** Reactive 2D viewport state (Svelte 5 runes). One instance per map editor. */
export class Viewport {
  panX = $state(0);
  panZ = $state(0);
  zoom = $state(1);

  // Last known canvas size in CSS pixels — kept so screen↔world is correct.
  width = $state(1);
  height = $state(1);

  get aspect(): number {
    return this.height > 0 ? this.width / this.height : 1;
  }

  /** World units currently visible (height, width) given the zoom + aspect. */
  visible(): { w: number; h: number } {
    const h = BASE_VIEW / this.zoom;
    return { w: h * this.aspect, h };
  }

  /** Pointer position (CSS px from the canvas top-left) → world point on the floor. */
  screenToWorld(px: number, py: number): { x: number; z: number } {
    const { w, h } = this.visible();
    return {
      x: this.panX + (px / this.width - 0.5) * w,
      z: this.panZ + (py / this.height - 0.5) * h,
    };
  }

  /** Drag the plan by a screen-space delta (CSS px). */
  panByPixels(dx: number, dy: number): void {
    const { w, h } = this.visible();
    this.panX -= (dx / this.width) * w;
    this.panZ -= (dy / this.height) * h;
  }

  /** Multiply zoom by `factor`, keeping the world point under (px,py) fixed. */
  zoomAt(px: number, py: number, factor: number): void {
    const before = this.screenToWorld(px, py);
    this.zoom = clampZoom(this.zoom * factor);
    const { w, h } = this.visible();
    this.panX = before.x - (px / this.width - 0.5) * w;
    this.panZ = before.z - (py / this.height - 0.5) * h;
  }

  /** Step zoom about the canvas centre (used by the +/- buttons). */
  zoomBy(factor: number): void {
    this.zoomAt(this.width / 2, this.height / 2, factor);
  }

  /** Frame the given world-space bounds with a little padding. */
  fit(b: Bounds | null): void {
    if (!b) {
      this.panX = 0;
      this.panZ = 0;
      this.zoom = 1;
      return;
    }
    const pad = 1.18;
    const needH = Math.max((b.maxZ - b.minZ) * pad, 4);
    const needW = Math.max((b.maxX - b.minX) * pad, 4);
    const zH = BASE_VIEW / needH;
    const zW = (BASE_VIEW * this.aspect) / needW;
    this.zoom = clampZoom(Math.min(zH, zW));
    this.panX = (b.minX + b.maxX) / 2;
    this.panZ = (b.minZ + b.maxZ) / 2;
  }

  /** Push this viewport onto a real orthographic camera (call when state/size change). */
  apply(cam: OrthographicCamera): void {
    const halfH = BASE_VIEW / 2;
    const halfW = halfH * this.aspect;
    cam.left = -halfW;
    cam.right = halfW;
    cam.top = halfH;
    cam.bottom = -halfH;
    cam.near = 0.1;
    cam.far = CAM_Y * 2;
    cam.zoom = this.zoom;
    cam.position.set(this.panX, CAM_Y, this.panZ);
    cam.up.set(0, 0, -1); // grid +y (down on the plan) → screen-down
    cam.lookAt(this.panX, 0, this.panZ);
    cam.updateProjectionMatrix();
  }
}

export { BASE_VIEW };
