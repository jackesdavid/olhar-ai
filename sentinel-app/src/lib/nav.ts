import type { Component } from "svelte";
import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
import Cpu from "@lucide/svelte/icons/cpu";
import Video from "@lucide/svelte/icons/video";
import Bell from "@lucide/svelte/icons/bell";
import Radio from "@lucide/svelte/icons/radio";
import Users from "@lucide/svelte/icons/users";
import Map from "@lucide/svelte/icons/map";
import MessageSquare from "@lucide/svelte/icons/message-square";
import Workflow from "@lucide/svelte/icons/workflow";
import ScrollText from "@lucide/svelte/icons/scroll-text";
import Clapperboard from "@lucide/svelte/icons/clapperboard";
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
  { label: "Alerts", href: "/alerts", icon: Bell },
  { label: "Recipients", href: "/beacons", icon: Radio },
  { label: "People", href: "/people", icon: Users },
  { label: "Map", href: "/map", icon: Map },
  { label: "Chat", href: "/chat", icon: MessageSquare },
  { label: "Workflows", href: "/workflows", icon: Workflow },
  { label: "Recordings", href: "/recordings", icon: Clapperboard },
  { label: "Logs", href: "/logs", icon: ScrollText },
  { label: "Settings", href: "/settings", icon: Settings },
];
