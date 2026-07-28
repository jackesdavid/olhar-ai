// App i18n runtime — a single reactive `$state` language, persisted to localStorage and
// browser-detected on first visit. Messages live in ./messages/<domain>.ts (one module per
// route/domain, each exporting `{ en, pt }`), composed into `dict` in ./index.ts. In a component:
//
//   import { dict, lang } from "$lib/i18n";
//   const t = $derived(dict[lang()]);        // then t.login.signin, t.common.save, …
//
// Reading `lang()` inside the `$derived` makes the whole component re-render on a language switch.

export type Lang = "en" | "pt";

let current = $state<Lang>("en");

/** Current language — reactive; read it inside a component/`$derived`. */
export function lang(): Lang {
  return current;
}

/**
 * BCP-47 locale for the current language — pass this to `Intl` / `toLocale*String` so dates,
 * numbers and currency follow the language the user picked, not whatever the browser is set to.
 */
export function locale(): string {
  return current === "pt" ? "pt-BR" : "en-US";
}

/** Switch language and persist the choice. */
export function setLang(l: Lang) {
  current = l;
  if (typeof localStorage !== "undefined") localStorage.setItem("lang", l);
  if (typeof document !== "undefined") document.documentElement.lang = l;
}

/** Resolve the initial language on the client: saved choice → browser → en. Call once on mount. */
export function initLang() {
  if (typeof window === "undefined") return;
  const saved = localStorage.getItem("lang");
  if (saved === "en" || saved === "pt") {
    setLang(saved);
    return;
  }
  setLang(navigator.language?.toLowerCase().startsWith("pt") ? "pt" : "en");
}
