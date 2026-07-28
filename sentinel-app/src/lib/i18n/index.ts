// Composes the per-domain message modules into one `dict` keyed by language, and re-exports the
// runtime. Add a new surface by creating ./messages/<domain>.ts (exporting `{ en, pt }`) and
// wiring it in here — nothing else changes.
//
//   import { dict, lang } from "$lib/i18n";
//   const t = $derived(dict[lang()]);

import type { Lang } from "./runtime.svelte";
import { common } from "./messages/common";
import { nav } from "./messages/nav";
import { login } from "./messages/login";

export { lang, setLang, initLang, type Lang } from "./runtime.svelte";

export const dict: Record<Lang, {
  common: (typeof common)["en"];
  nav: (typeof nav)["en"];
  login: (typeof login)["en"];
}> = {
  en: { common: common.en, nav: nav.en, login: login.en },
  pt: { common: common.pt, nav: nav.pt, login: login.pt },
};
