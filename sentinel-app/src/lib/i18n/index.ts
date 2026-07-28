// Composes the per-domain message modules into one `dict` keyed by language, and re-exports the
// runtime. Add a new surface by creating ./messages/<domain>.ts (exporting `{ en, pt }`) and
// wiring it in here — nothing else changes.
//
//   import { dict, lang } from "$lib/i18n";
//   const t = $derived(dict[lang()]);

import type { Lang } from "./runtime.svelte";
import { common } from "./messages/common";
import { nav } from "./messages/nav";
import { shell } from "./messages/shell";
import { login } from "./messages/login";
import { dashboard } from "./messages/dashboard";
import { live } from "./messages/live";

export { lang, setLang, initLang, type Lang } from "./runtime.svelte";

export const dict: Record<Lang, {
  common: (typeof common)["en"];
  nav: (typeof nav)["en"];
  shell: (typeof shell)["en"];
  login: (typeof login)["en"];
  dashboard: (typeof dashboard)["en"];
  live: (typeof live)["en"];
}> = {
  en: { common: common.en, nav: nav.en, shell: shell.en, login: login.en, dashboard: dashboard.en, live: live.en },
  pt: { common: common.pt, nav: nav.pt, shell: shell.pt, login: login.pt, dashboard: dashboard.pt, live: live.pt },
};
