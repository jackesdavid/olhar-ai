// Theme state: the app ships two operative themes — light "Clean White" (default)
// and dark "Sentinel Noir" — toggled by the `.dark` class on <html>. The initial
// class is set pre-paint by the inline script in app.html to avoid a flash; this
// store keeps the runtime toggle and localStorage in sync with that class.

export type Theme = "light" | "dark";

const THEME_KEY = "sentinel-theme";

export const theme = $state<{ current: Theme }>({ current: "light" });

/** Sync store state from whatever the pre-paint script already applied (call once on boot). */
export function loadTheme(): void {
  if (typeof document === "undefined") return;
  theme.current = document.documentElement.classList.contains("dark") ? "dark" : "light";
}

/** Apply a theme to <html>, persist it, and update the store. */
export function setTheme(next: Theme): void {
  theme.current = next;
  if (typeof document !== "undefined")
    document.documentElement.classList.toggle("dark", next === "dark");
  if (typeof localStorage !== "undefined") localStorage.setItem(THEME_KEY, next);
}

export function toggleTheme(): void {
  setTheme(theme.current === "dark" ? "light" : "dark");
}
