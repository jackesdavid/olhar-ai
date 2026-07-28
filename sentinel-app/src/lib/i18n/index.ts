// Composes the per-domain message modules into one `dict` keyed by language, and re-exports the
// runtime. Add a new surface by creating ./messages/<domain>.ts (exporting `{ en, pt }`) and
// adding it to `modules` below — two lines, nothing else changes.
//
//   import { dict, lang } from "$lib/i18n";
//   const t = $derived(dict[lang()].events);

import type { Lang } from "./runtime.svelte";
import { common } from "./messages/common";
import { nav } from "./messages/nav";
import { shell } from "./messages/shell";
import { login } from "./messages/login";
import { dashboard } from "./messages/dashboard";
import { live } from "./messages/live";
import { events } from "./messages/events";
import { people } from "./messages/people";
import { beacons } from "./messages/beacons";
import { settings } from "./messages/settings";
import { devices } from "./messages/devices";
import { recordings } from "./messages/recordings";
import { users } from "./messages/users";
import { roles } from "./messages/roles";
import { integrations } from "./messages/integrations";
import { billing } from "./messages/billing";
import { boxes } from "./messages/boxes";
import { boxInstall } from "./messages/boxInstall";
import { peopleDirectory } from "./messages/peopleDirectory";
import { personDetail } from "./messages/personDetail";
import { enroll } from "./messages/enroll";

export { lang, setLang, initLang, locale, type Lang } from "./runtime.svelte";

/**
 * Pins a module's `pt` to its `en` shape, so a key that exists in English but was never
 * translated is a compile error here rather than a silent `undefined` on screen. `NoInfer`
 * makes `en` the sole source of the shape — otherwise TS would quietly settle on whatever
 * the two languages have in common and the error would surface at some far-off call site.
 */
function domain<T extends Record<string, unknown>>(m: { en: T; pt: NoInfer<T> }): { en: T; pt: T } {
  return m;
}

const modules = {
  common: domain(common),
  nav: domain(nav),
  shell: domain(shell),
  login: domain(login),
  dashboard: domain(dashboard),
  live: domain(live),
  events: domain(events),
  people: domain(people),
  beacons: domain(beacons),
  settings: domain(settings),
  devices: domain(devices),
  recordings: domain(recordings),
  users: domain(users),
  roles: domain(roles),
  integrations: domain(integrations),
  billing: domain(billing),
  boxes: domain(boxes),
  boxInstall: domain(boxInstall),
  peopleDirectory: domain(peopleDirectory),
  personDetail: domain(personDetail),
  enroll: domain(enroll),
};

/** One language's messages — every module, narrowed to that language. */
export type Messages = { [K in keyof typeof modules]: (typeof modules)[K]["en"] };

function forLang(l: Lang): Messages {
  const out = Object.entries(modules).map(([key, m]) => [key, m[l]]);
  return Object.fromEntries(out) as Messages;
}

export const dict: Record<Lang, Messages> = { en: forLang("en"), pt: forLang("pt") };
