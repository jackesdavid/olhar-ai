// Undo/redo over the floor-plan document. Snapshots are deep clones of the
// { rooms, walls, markers } document; the store checkpoints before discrete edits
// (and once at the start of a drag gesture) and applies snapshots on undo/redo.
import type { MapDoc } from "$lib/types";

export function cloneDoc(doc: MapDoc): MapDoc {
  return structuredClone($state.snapshot(doc) as MapDoc);
}

export class History {
  private undoStack: MapDoc[] = [];
  private redoStack: MapDoc[] = [];
  private cap = 60;

  /** Record `current` as a restore point; clears the redo branch. */
  push(current: MapDoc): void {
    this.undoStack.push(cloneDoc(current));
    if (this.undoStack.length > this.cap) this.undoStack.shift();
    this.redoStack = [];
  }

  get canUndo(): boolean {
    return this.undoStack.length > 0;
  }
  get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  /** Pop the last restore point, stashing `current` for redo. */
  undo(current: MapDoc): MapDoc | null {
    const prev = this.undoStack.pop();
    if (!prev) return null;
    this.redoStack.push(cloneDoc(current));
    return prev;
  }

  /** Re-apply the last undone state, stashing `current` for undo. */
  redo(current: MapDoc): MapDoc | null {
    const next = this.redoStack.pop();
    if (!next) return null;
    this.undoStack.push(cloneDoc(current));
    return next;
  }
}
