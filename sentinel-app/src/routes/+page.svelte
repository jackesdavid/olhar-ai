<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import { onMount } from "svelte";
  import { data } from "$lib/data";

  import Cpu from "@lucide/svelte/icons/cpu";
  import Bell from "@lucide/svelte/icons/bell";
  import Video from "@lucide/svelte/icons/video";
  import Radio from "@lucide/svelte/icons/radio";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Workflow from "@lucide/svelte/icons/workflow";
  import Map from "@lucide/svelte/icons/map";

  type Device = { rtsp_url?: string | null };
  type Alert = { deliveries?: { ok: boolean }[] };
  type Target = { enabled: boolean };

  let devices = $state<Device[]>([]);
  let alerts = $state<Alert[]>([]);
  let recipients = $state<Target[]>([]);
  let loaded = $state(false);

  const camerasReady = $derived(devices.filter((d) => d.rtsp_url && d.rtsp_url.trim()).length);
  const openAlerts = $derived(alerts.filter((a) => !(a.deliveries ?? []).some((d) => d.ok)).length);
  const liveRecipients = $derived(recipients.filter((r) => r.enabled).length);

  const stats = $derived([
    { title: "Devices", value: devices.length, icon: Cpu, href: "/devices" },
    { title: "Cameras live", value: camerasReady, icon: Video, href: "/live" },
    { title: "Open alerts", value: openAlerts, icon: Bell, href: "/alerts" },
    { title: "Recipients", value: liveRecipients, icon: Radio, href: "/beacons" },
  ]);

  const actions = [
    { title: "Ask Sentinel", caption: "Chat with the assistant", icon: MessageSquare, href: "/chat" },
    { title: "Build a workflow", caption: "Automate a response", icon: Workflow, href: "/workflows" },
    { title: "Open the map", caption: "See your floor plan", icon: Map, href: "/map" },
    { title: "Watch live", caption: "Real-time camera video", icon: Video, href: "/live" },
  ];

  onMount(async () => {
    try {
      const [d, a, r] = await Promise.all([
        data.listDevices().catch(() => []),
        data.listAlerts().catch(() => []),
        data.listBeacons().catch(() => []),
      ]);
      devices = d;
      alerts = a;
      recipients = r;
    } finally {
      loaded = true;
    }
  });
</script>

<div class="space-y-8">
  <PageHeader
    title="Dashboard"
    description="Overview of your devices, alerts, and automations."
    icon={LayoutDashboard}
  >
    {#snippet actions()}
      <div
        class="flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3 py-1 text-xs font-medium text-primary"
      >
        <span class="size-1.5 rounded-full bg-primary status-live"></span>
        System ready
      </div>
    {/snippet}
  </PageHeader>

  <!-- Stat cards -->
  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
    {#each stats as stat (stat.title)}
      {@const Icon = stat.icon}
      <a
        href={stat.href}
        class="interactive group block rounded-xl border border-border bg-card p-5"
      >
        <div class="flex items-center justify-between">
          <div
            class="flex size-10 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-inset ring-primary/20"
          >
            <Icon class="size-5" />
          </div>
          <ArrowUpRight
            class="size-4 -translate-x-1 text-muted-foreground opacity-0 transition-all duration-200 group-hover:translate-x-0 group-hover:opacity-100"
          />
        </div>
        <div class="mt-4 text-3xl font-semibold tabular-nums tracking-tight">
          {#if loaded}{stat.value}{:else}<span class="text-muted-foreground/40">—</span>{/if}
        </div>
        <p class="mt-1 text-sm text-muted-foreground">{stat.title}</p>
      </a>
    {/each}
  </div>

  <!-- Quick actions -->
  <div class="space-y-3">
    <h2 class="text-sm font-medium text-muted-foreground">Quick actions</h2>
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {#each actions as action (action.href)}
        {@const Icon = action.icon}
        <a
          href={action.href}
          class="interactive group flex items-start gap-3 rounded-xl border border-border bg-card/60 p-4"
        >
          <div
            class="flex size-9 items-center justify-center rounded-lg bg-muted text-muted-foreground transition-colors group-hover:bg-primary/15 group-hover:text-primary"
          >
            <Icon class="size-4.5" />
          </div>
          <div class="min-w-0">
            <div class="text-sm font-medium">{action.title}</div>
            <div class="truncate text-xs text-muted-foreground">{action.caption}</div>
          </div>
        </a>
      {/each}
    </div>
  </div>
</div>
