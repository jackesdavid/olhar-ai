// Theme selection. The web app ships two operative themes — light "Clean White" and dark
// "Sentinel Noir" — and lets the user pick explicitly rather than following the OS
// (`sentinel-app/src/lib/theme.svelte.ts`). The phone mirrors that choice, and adds "system" as the
// default because that is the native expectation on mobile.
//
// The preference is an external store (same pattern as the session token) so every screen re-renders
// the moment it changes — a theme that only applies after a restart reads as broken.
import { useEffect, useSyncExternalStore } from "react";
import { useColorScheme } from "react-native";
import * as SecureStore from "expo-secure-store";
import { darkTheme, lightTheme, type Theme } from "./tokens";

export type ThemePref = "system" | "light" | "dark";

const PREF_KEY = "olharai.theme";

let pref: ThemePref = "system";
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function snapshot(): ThemePref {
  return pref;
}

/** Load the stored preference once on boot. Safe to call from a layout effect. */
export async function loadThemePref(): Promise<void> {
  const stored = await SecureStore.getItemAsync(PREF_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") {
    pref = stored;
    emit();
  }
}

export function setThemePref(next: ThemePref): void {
  pref = next;
  emit();
  void SecureStore.setItemAsync(PREF_KEY, next);
}

export function useThemePref(): ThemePref {
  return useSyncExternalStore(subscribe, snapshot);
}

/** The resolved palette: the explicit choice, or the OS setting when following the system. */
export function useTheme(): Theme {
  const chosen = useThemePref();
  const os = useColorScheme();
  const dark = chosen === "system" ? os === "dark" : chosen === "dark";
  return dark ? darkTheme : lightTheme;
}

/** Convenience for the root layout: load the stored preference on mount. */
export function useLoadThemePref(): void {
  useEffect(() => {
    void loadThemePref();
  }, []);
}

export { radius, space, type as typeScale } from "./tokens";
export type { Theme };
