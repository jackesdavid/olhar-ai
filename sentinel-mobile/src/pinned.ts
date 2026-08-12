// Which workflows the phone puts on its home screen (card #178).
//
// A device-local choice, not a tenant setting: the workflows *I* want one tap away are mine, and
// the same account on another phone will want different ones. Stored on the device for that reason,
// and read by both the Run screen and the widget.
import { useEffect, useState } from "react";
import * as SecureStore from "expo-secure-store";

const KEY = "sentinel.pinned_workflows";

let cache: string[] = [];
let loaded = false;
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((l) => l());
}

async function load(): Promise<string[]> {
  if (loaded) return cache;
  try {
    const raw = await SecureStore.getItemAsync(KEY);
    cache = raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    cache = [];
  }
  loaded = true;
  emit();
  return cache;
}

async function save(next: string[]): Promise<void> {
  cache = next;
  emit();
  try {
    await SecureStore.setItemAsync(KEY, JSON.stringify(next));
  } catch {
    // A failed write costs a preference, not a workflow — never block the tap on it.
  }
}

export function isPinned(id: string): boolean {
  return cache.includes(id);
}

export function togglePinned(id: string): void {
  void save(cache.includes(id) ? cache.filter((x) => x !== id) : [...cache, id]);
}

/** The pinned ids, re-rendering the caller when they change. */
export function usePinned(): string[] {
  const [ids, setIds] = useState<string[]>(cache);
  useEffect(() => {
    const listener = () => setIds([...cache]);
    listeners.add(listener);
    void load();
    return () => {
      listeners.delete(listener);
    };
  }, []);
  return ids;
}
