import type { Component } from "svelte";
import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
import Video from "@lucide/svelte/icons/video";
import Rss from "@lucide/svelte/icons/rss";
import Radio from "@lucide/svelte/icons/radio";
import Users from "@lucide/svelte/icons/users";
import ScanFace from "@lucide/svelte/icons/scan-face";
import Map from "@lucide/svelte/icons/map";
import MessageSquare from "@lucide/svelte/icons/message-square";
import Workflow from "@lucide/svelte/icons/workflow";
import ScrollText from "@lucide/svelte/icons/scroll-text";
import Clapperboard from "@lucide/svelte/icons/clapperboard";
import Film from "@lucide/svelte/icons/film";

import type { nav } from "$lib/i18n/messages/nav";

/** A key into the `nav` message module — the label is resolved at render time, not here. */
export type NavKey = keyof (typeof nav)["en"];

export type NavItem = {
  key: NavKey;
  href: string;
  icon: Component;
};

// Devices + Settings live in the sidebar's Settings group now (see +layout.svelte
// `settingsItems`), not the main menu.
export const navItems: NavItem[] = [
  { key: "dashboard", href: "/", icon: LayoutDashboard },
  { key: "live", href: "/live", icon: Video },
  { key: "events", href: "/events", icon: Rss },
  { key: "recipients", href: "/beacons", icon: Radio },
  { key: "people", href: "/people", icon: Users },
  { key: "faces", href: "/faces", icon: ScanFace },
  { key: "map", href: "/map", icon: Map },
  { key: "chat", href: "/chat", icon: MessageSquare },
  { key: "workflows", href: "/workflows", icon: Workflow },
  { key: "recordings", href: "/recordings", icon: Clapperboard },
  { key: "playback", href: "/playback", icon: Film },
  { key: "logs", href: "/logs", icon: ScrollText },
];
