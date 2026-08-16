<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import { onMount } from "svelte";
  import { data } from "$lib/data";
  import { boxes as boxesApi } from "$lib/api";
  import { dict, lang } from "$lib/i18n";
  import { Button } from "$lib/components/ui/button";

  import Cpu from "@lucide/svelte/icons/cpu";
  import Bell from "@lucide/svelte/icons/bell";
  import Video from "@lucide/svelte/icons/video";
  import Radio from "@lucide/svelte/icons/radio";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Workflow from "@lucide/svelte/icons/workflow";
  import Map from "@lucide/svelte/icons/map";

  type Device = { rtsp_url?: string | null };
  type Box = { id: string };
  type Alert = { deliveries?: { ok: boolean }[] };
  type Target = { enabled: boolean };

  const t = $derived(dict[lang()].dashboard);

  let devices = $state<Device[]>([]);
  let alerts = $state<Alert[]>([]);
  let recipients = $state<Target[]>([]);
  let boxList = $state<Box[]>([]);
  let loaded = $state(false);
  // A load that partly failed is NOT an empty account, and the difference is the whole point of
  // this flag. Three `.catch(() => [])` used to turn a control-plane outage into "0 of everything,
  // System ready" — a false claim rendered confidently.
  let untrusted = $state(false);

  const camerasReady = $derived(devices.filter((d) => d.rtsp_url && d.rtsp_url.trim()).length);
  const openAlerts = $derived(alerts.filter((a) => !(a.deliveries ?? []).some((d) => d.ok)).length);
  const liveRecipients = $derived(recipients.filter((r) => r.enabled).length);

  const stats = $derived([
    { title: t.statDevices, value: devices.length, icon: Cpu, href: "/settings/devices" },
    { title: t.statCamerasLive, value: camerasReady, icon: Video, href: "/live" },
    { title: t.statOpenAlerts, value: openAlerts, icon: Bell, href: "/events?kind=alert" },
    { title: t.statRecipients, value: liveRecipients, icon: Radio, href: "/beacons" },
  ]);

  const actions = $derived([
    { title: t.askTitle, caption: t.askCaption, icon: MessageSquare, href: "/chat" },
    { title: t.workflowTitle, caption: t.workflowCaption, icon: Workflow, href: "/workflows" },
    { title: t.mapTitle, caption: t.mapCaption, icon: Map, href: "/map" },
    { title: t.liveTitle, caption: t.liveCaption, icon: Video, href: "/live" },
  ]);

  async function load() {
    loaded = false;
    untrusted = false;
    // `allSettled`, so one failure does not blank the rest — but it is RECORDED rather than
    // swallowed. Anything that failed leaves the view marked untrusted.
    const results = await Promise.allSettled([
      data.listDevices(),
      data.listAlerts(),
      data.listBeacons(),
      boxesApi.list(),
    ]);
    const [d, a, r, b] = results;
    devices = d.status === "fulfilled" ? d.value : [];
    alerts = a.status === "fulfilled" ? a.value : [];
    recipients = r.status === "fulfilled" ? r.value : [];
    boxList = b.status === "fulfilled" ? (b.value as Box[]) : [];
    untrusted = results.some((x) => x.status === "rejected");
    loaded = true;
  }

  onMount(load);

  /** No appliance means the numbers below are all structurally zero, and every quick action leads
   *  somewhere that cannot work yet. Show the one next step instead. */
  function needsSetup(): boolean {
    return loaded && !untrusted && boxList.length === 0;
  }
  /** "System ready" is a claim. It is only true once something is actually connected. */
  function systemIsReady(): boolean {
    return loaded && !untrusted && boxList.length > 0;
  }
</script>

<div class="space-y-8">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={LayoutDashboard}
  >
    {#snippet actions()}
      {#if systemIsReady()}
        <div
          class="flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3 py-1 text-xs font-medium text-primary"
        >
          <span class="size-1.5 rounded-full bg-primary status-live"></span>
          {t.systemReady}
        </div>
      {/if}
    {/snippet}
  </PageHeader>

  {#if untrusted}
    <!-- Said plainly, and owned: "this is our side, not yours". A person who thinks they broke it
         goes looking for their own mistake. -->
    <div class="space-y-2 rounded-xl border border-destructive/40 bg-card p-5">
      <p class="font-medium">{t.loadFailedTitle}</p>
      <p class="text-sm text-muted-foreground">{t.loadFailedBody}</p>
      <Button size="sm" variant="outline" onclick={load}>{t.retry}</Button>
    </div>
  {/if}

  {#if needsSetup()}
    <!-- The first screen after signup. One next step, and the reason behind it — someone about to
         put cameras on their home network deserves to know what leaves it. -->
    <div class="space-y-3 rounded-xl border border-border bg-card p-6">
      <h2 class="text-lg font-semibold">{t.setupTitle}</h2>
      <p class="max-w-2xl text-sm text-muted-foreground">{t.setupBody}</p>
      <!-- The LIST, not /install: install bounces a user with no box straight back out. Creating a
           box on the list flows into the install view. -->
      <Button href="/settings/boxes">{t.setupCta}</Button>
    </div>
  {/if}

  <!-- Stat cards -->
  <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
    {#each stats as stat (stat.href)}
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
    <h2 class="text-sm font-medium text-muted-foreground">{t.quickActions}</h2>
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {#each actions as action (action.href)}
        {@const Icon = action.icon}
        <a
          href={action.href}
          class="interactive group flex items-start gap-3 rounded-xl border border-border bg-card p-4"
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
