import type { Component } from "svelte";
import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
import Cpu from "@lucide/svelte/icons/cpu";
import Video from "@lucide/svelte/icons/video";
import Rss from "@lucide/svelte/icons/rss";
import Radio from "@lucide/svelte/icons/radio";
import Users from "@lucide/svelte/icons/users";
import Map from "@lucide/svelte/icons/map";
import MessageSquare from "@lucide/svelte/icons/message-square";
import Workflow from "@lucide/svelte/icons/workflow";
import ScrollText from "@lucide/svelte/icons/scroll-text";
import Clapperboard from "@lucide/svelte/icons/clapperboard";
import Film from "@lucide/svelte/icons/film";
import Settings from "@lucide/svelte/icons/settings";

export type NavItem = {
  label: string;
  href: string;
  icon: Component;
};

export const navItems: NavItem[] = [
  { label: "Dashboard", href: "/", icon: LayoutDashboard },
  { label: "Devices", href: "/devices", icon: Cpu },
  { label: "Live", href: "/live", icon: Video },
  { label: "Events", href: "/events", icon: Rss },
  { label: "Recipients", href: "/beacons", icon: Radio },
  { label: "People", href: "/people", icon: Users },
  { label: "Map", href: "/map", icon: Map },
  { label: "Chat", href: "/chat", icon: MessageSquare },
  { label: "Workflows", href: "/workflows", icon: Workflow },
  { label: "Recordings", href: "/recordings", icon: Clapperboard },
  { label: "Playback", href: "/playback", icon: Film },
  { label: "Logs", href: "/logs", icon: ScrollText },
  { label: "Settings", href: "/settings", icon: Settings },
];
