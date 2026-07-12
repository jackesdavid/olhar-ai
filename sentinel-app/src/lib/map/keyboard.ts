// Keyboard commands for the map editor. Kept out of the page so the route shell
// stays thin. Returns true if the event was handled (so the caller can stop here).
import type { MapStore } from "./store.svelte";
import type { ToolState } from "./tools.svelte";

type Ctx = {
  store: MapStore;
  tools: ToolState;
  fit: () => void;
  toggleHelp?: () => void;
  togglePanel?: () => void;
};

const ARROW: Record<string, { dx: number; dy: number; edge: "left" | "right" | "top" | "bottom" }> = {
  ArrowLeft: { dx: -1, dy: 0, edge: "left" },
  ArrowRight: { dx: 1, dy: 0, edge: "right" },
  ArrowUp: { dx: 0, dy: -1, edge: "top" },
  ArrowDown: { dx: 0, dy: 1, edge: "bottom" },
};

function typingInto(): boolean {
  const el = document.activeElement as HTMLElement | null;
  const tag = el?.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || !!el?.isContentEditable;
}

// Tab is THE focus-navigation key, so we only claim it when focus isn't on a control —
// otherwise the editor becomes a keyboard trap you can't Tab out of.
function onControl(): boolean {
  const el = document.activeElement as HTMLElement | null;
  return !!el?.closest("button, a[href], [tabindex]:not([tabindex='-1'])");
}

export function handleMapKey(e: KeyboardEvent, ctx: Ctx): boolean {
  if (typingInto()) return false;
  const { store, tools, fit, toggleHelp, togglePanel } = ctx;

  // Tab collapses/expands the side panel — the same gesture as the workflow editor.
  // (It used to cycle the selection; that moved to C/⇧C.)
  if (e.key === "Tab") {
    if (onControl()) return false; // let the browser move focus
    e.preventDefault();
    togglePanel?.();
    return true;
  }

  // rotate the selection: [ = CCW, ] = CW (Shift = fine 1° step)
  if (e.key === "[" || e.key === "]") {
    e.preventDefault();
    const step = e.shiftKey ? 1 : 15;
    store.rotateSelected(e.key === "[" ? -step : step);
    return true;
  }

  // shortcuts overlay
  if (e.key === "?") {
    e.preventDefault();
    toggleHelp?.();
    return true;
  }

  // undo / redo
  if ((e.metaKey || e.ctrlKey) && (e.key === "z" || e.key === "Z")) {
    e.preventDefault();
    if (e.shiftKey) store.redo();
    else store.undo();
    return true;
  }

  const arrow = ARROW[e.key];
  if (arrow) {
    e.preventDefault();
    if (e.shiftKey && store.selectedRoom) {
      // grow that edge; Alt to shrink it
      store.resizeSelectedRoom(arrow.edge, !e.altKey);
    } else {
      store.nudgeSelected(arrow.dx, arrow.dy);
    }
    return true;
  }

  switch (e.key) {
    case "Escape":
      tools.cancel();
      store.clearSelection();
      return true;
    // Cycle selection. This lived on Tab/⇧Tab; Tab now toggles the side panel in BOTH
    // editors (the workflow editor uses it for the same gesture), so cycling moved to C/⇧C.
    case "c":
    case "C":
      store.cycleSelection(e.shiftKey ? -1 : 1);
      return true;
    case "Delete":
    case "Backspace":
      store.deleteSelected();
      return true;
    case "v":
    case "V":
      tools.setMode("select");
      return true;
    case "w":
    case "W":
      tools.setMode("wall");
      return true;
    case "m":
    case "M":
      tools.setMode("marker");
      return true;
    case "f":
    case "F":
      fit();
      return true;
    default:
      return false;
  }
}
