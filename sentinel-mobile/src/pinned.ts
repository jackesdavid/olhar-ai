// Which workflows the phone puts on its home screen (card #178).
//
// A device-local choice, not a tenant setting: the workflows *I* want one tap away are mine, and
// the same account on another phone will want different ones.
//
// Two readers, one truth. The Run screen reads it from here; the home-screen widget reads it out of
// the shared App Group container, which this module writes on every change. The widget is a
// separate process with its own sandbox — it cannot see SecureStore and must never hold a session
// token — so {id, name} pairs are all that crosses. No credentials, no API, just labels and the ids
// its buttons deep-link to.
import { useEffect, useState } from "react";
import * as SecureStore from "expo-secure-store";
import { ExtensionStorage } from "@bacons/apple-targets";

const KEY = "sentinel.pinned_workflows";
/** Must match `SharedWorkflows.suite` / `.key` in `targets/widget/OlharAIWidget.swift`. */
const APP_GROUP = "group.com.olharai.app";
/** EVERY workflow, because the widget's "Edit Widget" picker offers this list. Starring orders the
 *  Run tab; it does not decide what a widget can point at — the home screen chooses for itself. */
const WIDGET_KEY = "workflows";

/** What the widget needs to draw a button: a label to show and an id to deep-link. */
export type Pin = { id: string; name: string };

let cache: Pin[] = [];
let loaded = false;
const listeners = new Set<() => void>();
const shared = new ExtensionStorage(APP_GROUP);

function emit() {
  listeners.forEach((l) => l());
}

/** Everything the widget picker can offer. Starred first, so the default pick is a favourite. */
let published: Pin[] = [];

/** Push the workflow list to the widget and ask iOS to redraw it. */
function publish(): void {
  try {
    const starredFirst = [
      ...published.filter((w) => cache.some((p) => p.id === w.id)),
      ...published.filter((w) => !cache.some((p) => p.id === w.id)),
    ];
    shared.set(WIDGET_KEY, JSON.stringify(starredFirst));
    // Without this the tile keeps yesterday's label until iOS decides to refresh on its own.
    ExtensionStorage.reloadWidget();
  } catch {
    // A widget that missed an update is a stale tile, not a broken app — never throw into a tap.
  }
}

/** Hand the widget the full list of workflows (called whenever the Run tab loads). */
export function publishWorkflows(all: Pin[]): void {
  published = all;
  publish();
}

async function load(): Promise<Pin[]> {
  if (loaded) return cache;
  try {
    const raw = await SecureStore.getItemAsync(KEY);
    cache = raw ? (JSON.parse(raw) as Pin[]) : [];
  } catch {
    cache = [];
  }
  loaded = true;
  emit();
  publish();
  return cache;
}

async function save(next: Pin[]): Promise<void> {
  cache = next;
  emit();
  publish();
  try {
    await SecureStore.setItemAsync(KEY, JSON.stringify(next));
  } catch {
    // A failed write costs a preference, not a workflow.
  }
}

export function isPinned(id: string): boolean {
  return cache.some((p) => p.id === id);
}

/** Star / unstar. The NAME is stored too — the widget has no way to look one up. */
export function togglePinned(pin: Pin): void {
  const has = cache.some((p) => p.id === pin.id);
  void save(has ? cache.filter((p) => p.id !== pin.id) : [...cache, pin]);
}

/** Keep a starred label current when a workflow is renamed on the web. */
export function syncNames(known: Pin[]): void {
  publishWorkflows(known);
  const byId = new Map(known.map((k) => [k.id, k.name]));
  let changed = false;
  const next = cache.map((p) => {
    const name = byId.get(p.id);
    if (name && name !== p.name) {
      changed = true;
      return { ...p, name };
    }
    return p;
  });
  if (changed) void save(next);
}

/** The starred workflows, re-rendering the caller when they change. */
export function usePinned(): Pin[] {
  const [pins, setPins] = useState<Pin[]>(cache);
  useEffect(() => {
    const listener = () => setPins([...cache]);
    listeners.add(listener);
    void load();
    return () => {
      listeners.delete(listener);
    };
  }, []);
  return pins;
}
