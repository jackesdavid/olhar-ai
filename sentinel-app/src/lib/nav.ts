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

export type NavItem = {
  label: string;
  href: string;
  icon: Component;
};

// Devices + Settings live in the sidebar's Settings group now (see +layout.svelte
// `settingsItems`), not the main menu.
export const navItems: NavItem[] = [
  { label: "Dashboard", href: "/", icon: LayoutDashboard },
  { label: "Live", href: "/live", icon: Video },
  { label: "Events", href: "/events", icon: Rss },
  { label: "Recipients", href: "/beacons", icon: Radio },
  { label: "People", href: "/people", icon: Users },
  { label: "Faces", href: "/faces", icon: ScanFace },
  { label: "Map", href: "/map", icon: Map },
  { label: "Chat", href: "/chat", icon: MessageSquare },
  { label: "Workflows", href: "/workflows", icon: Workflow },
  { label: "Recordings", href: "/recordings", icon: Clapperboard },
  { label: "Playback", href: "/playback", icon: Film },
  { label: "Logs", href: "/logs", icon: ScrollText },
];
