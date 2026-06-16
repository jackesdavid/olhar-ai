<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { data, onEvent } from "$lib/data";
  import { COMMIT } from "$lib/config";
  import type { Alert } from "$lib/types";
  import { navItems } from "$lib/nav";
  import { Toaster } from "$lib/components/ui/sonner";
  import SentinelMark from "$lib/components/sentinel-mark.svelte";
  import SensorField from "$lib/components/sensor-field.svelte";
  import Splash from "$lib/components/splash.svelte";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import UsersRound from "@lucide/svelte/icons/users-round";
  import Building2 from "@lucide/svelte/icons/building-2";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import LogOut from "@lucide/svelte/icons/log-out";
  import Undo2 from "@lucide/svelte/icons/undo-2";

  import {
    auth,
    loadToken,
    loadMe,
    logout,
    hasPerm,
    isAssuming,
    exitAssume,
  } from "$lib/auth.svelte";

  let { children } = $props();

  // Boot gate: hold the splash until the JWT session check completes.
  let ready = $state(false); // app content mounted
  let showSplash = $state(true); // splash present
  let leaving = $state(false); // splash fading out
  let bootState = $state<"connecting" | "error">("connecting");

  // ---- boot: require a valid JWT session ----------------------------------
  async function cloudBoot() {
    loadToken();
    const path = $page.url.pathname;
    const finish = () => {
      ready = true;
      leaving = true;
      showSplash = false;
    };
    if (!auth.token) {
      auth.ready = true;
      finish();
      if (path !== "/login") await goto("/login");
      return;
    }
    const ok = await loadMe();
    auth.ready = true;
    if (!ok) {
      logout();
      finish();
      if (path !== "/login") await goto("/login");
      return;
    }
    finish();
    // A master's own token has no tenant context — tenant-scoped pages would 400.
    // Send them to the console to pick a tenant to operate inside (Navigate as tenant).
    if (auth.me?.is_master && !auth.me.tenant_id && path !== "/admin/tenants") {
      await goto("/admin/tenants");
    }
  }

  let navCollapsed = $state(false);
  function toggleNav() {
    navCollapsed = !navCollapsed;
    try {
      localStorage.setItem("sentinel.navCollapsed", String(navCollapsed));
    } catch {
      /* ignore */
    }
  }
  // App-wide alert toasts: the relay forwards `alerts-changed` (tenant-scoped) whenever an
  // alert is raised/delivered; surface anything newer than the baseline as a toast, anywhere.
  function toastAlert(a: Alert) {
    const title = `New ${a.level} alert`;
    const opts = { description: a.room_name ? `${a.message} — ${a.room_name}` : a.message };
    if (a.level === "error") toast.error(title, opts);
    else if (a.level === "warn") toast.warning(title, opts);
    else toast.info(title, opts);
  }

  onMount(() => {
    if (localStorage.getItem("sentinel.navCollapsed") === "true") navCollapsed = true;
    cloudBoot();
  });

  // App-wide alert toasts — only once authenticated. Subscribing earlier would fire
  // unauthenticated `/alerts` calls that 401; the api layer's 401 handler then clears the
  // session and bounces to /login, which on an OAuth return clobbers the token that the
  // login page just set (it races the same mount). Gate on `auth.me` so nothing
  // authenticated runs until we have a session.
  $effect(() => {
    if (!auth.me) return;
    let lastAlertId = 0;
    // Baseline now so we only toast alerts that arrive after this point.
    data.listAlerts().then((a) => {
      lastAlertId = a.reduce((m, x) => Math.max(m, x.id), 0);
    }).catch(() => {});
    let unsub: (() => void) | null = null;
    onEvent("alerts-changed", async () => {
      try {
        const alerts = await data.listAlerts();
        const newest = alerts.reduce<Alert | null>((m, x) => (!m || x.id > m.id ? x : m), null);
        if (newest && newest.id > lastAlertId) {
          lastAlertId = newest.id;
          toastAlert(newest);
        }
      } catch {
        /* ignore */
      }
    }).then((u) => {
      unsub = u;
    });
    return () => unsub?.();
  });

  function isActive(href: string): boolean {
    const path = $page.url.pathname;
    if (href === "/") return path === "/";
    return path === href || path.startsWith(href + "/");
  }

  // Routes that manage their own full-width/height canvas (no centered max-width
  // container, no page padding) — e.g. the workflow editor.
  const FULL_BLEED = ["/workflows"];
  const fullBleed = $derived(
    FULL_BLEED.some((p) => $page.url.pathname === p || $page.url.pathname.startsWith(p + "/")),
  );

  // The app shell (sidebar + chrome) shows only once authenticated. The bare-render
  // path (no shell) carries /login.
  const showShell = $derived(auth.me !== null);

  // Per-page read permission (cloud mode filters the nav by these). `null` = always shown.
  const NAV_PERM: Record<string, string | null> = {
    "/": null,
    "/devices": "devices.read",
    "/live": "live.view",
    "/alerts": "alerts.read",
    "/beacons": "beacons.read",
    "/people": "persons.read",
    "/map": "map.read",
    "/chat": null,
    "/workflows": "workflows.read",
    "/logs": "devices.read",
    "/settings": "settings.read",
  };
  // Features without a cloud backend yet (the AI agent) — kept as routes but hidden from nav.
  const CLOUD_HIDE = new Set(["/chat"]);
  // A tenant context exists for normal users always, and for the master only while
  // "acting as" a tenant (assume sets tenant_id). Tenant-scoped pages require it.
  const inTenant = $derived(auth.me?.tenant_id != null);
  const mainNav = $derived(
    !inTenant
      ? []
      : navItems.filter(
          (i) => !CLOUD_HIDE.has(i.href) && (NAV_PERM[i.href] == null || hasPerm(NAV_PERM[i.href]!)),
        ),
  );

  // Tenant-admin screens, shown by permission (cloud mode only — `me` is null on desktop).
  const adminItems = $derived([
    ...(inTenant && hasPerm("devices.write") ? [{ label: "Edge boxes", href: "/settings/boxes", icon: HardDrive }] : []),
    ...(inTenant && hasPerm("roles.manage") ? [{ label: "Roles", href: "/settings/roles", icon: ShieldCheck }] : []),
    ...(inTenant && hasPerm("users.manage") ? [{ label: "Users", href: "/settings/users", icon: UsersRound }] : []),
    ...(auth.me?.is_master ? [{ label: "Tenants", href: "/admin/tenants", icon: Building2 }] : []),
  ]);

  async function signOut() {
    logout();
    await goto("/login");
  }
  async function leaveTenant() {
    await exitAssume();
    await goto("/admin/tenants");
  }
</script>

<SensorField />

{#if ready}
{#if showShell}
<div class="flex h-screen w-screen overflow-hidden text-foreground">
  <aside
    class="flex shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground transition-[width] duration-200 {navCollapsed
      ? 'w-16'
      : 'w-60'}"
  >
    <div
      class="flex h-14 items-center gap-2.5 border-b border-sidebar-border {navCollapsed
        ? 'justify-center px-0'
        : 'px-4'}"
    >
      <SentinelMark
        size={32}
        class="shrink-0 text-primary drop-shadow-[0_0_10px_oklch(0.7_0.16_162/0.45)]"
      />
      {#if !navCollapsed}
        <div class="flex flex-col leading-tight">
          <span class="text-sm font-semibold tracking-tight">Sentinel</span>
          <span class="text-[10px] text-muted-foreground">Monitoring &amp; Automation</span>
        </div>
      {/if}
    </div>

    <nav class="flex-1 space-y-0.5 overflow-y-auto p-2">
      {#each mainNav as item (item.href)}
        {@const Icon = item.icon}
        {@const active = isActive(item.href)}
        <a
          href={item.href}
          data-active={active}
          title={navCollapsed ? item.label : undefined}
          class="group relative flex items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground {navCollapsed
            ? 'justify-center px-0'
            : 'px-3'}"
        >
          <span
            class="absolute left-0 top-1/2 h-5 -translate-y-1/2 rounded-r-full bg-primary transition-all duration-200 {active
              ? 'w-1 opacity-100'
              : 'w-0 opacity-0'}"
          ></span>
          <Icon class="size-4 shrink-0 transition-colors group-data-[active=true]:text-primary" />
          {#if !navCollapsed}<span>{item.label}</span>{/if}
        </a>
      {/each}

      {#if adminItems.length > 0}
        <div class="px-3 pb-1 pt-3 {navCollapsed ? 'hidden' : ''}">
          <span class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Admin</span>
        </div>
        {#each adminItems as item (item.href)}
          {@const Icon = item.icon}
          {@const active = isActive(item.href)}
          <a
            href={item.href}
            data-active={active}
            title={navCollapsed ? item.label : undefined}
            class="group relative flex items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground {navCollapsed
              ? 'justify-center px-0'
              : 'px-3'}"
          >
            <span
              class="absolute left-0 top-1/2 h-5 -translate-y-1/2 rounded-r-full bg-primary transition-all duration-200 {active
                ? 'w-1 opacity-100'
                : 'w-0 opacity-0'}"
            ></span>
            <Icon class="size-4 shrink-0 transition-colors group-data-[active=true]:text-primary" />
            {#if !navCollapsed}<span>{item.label}</span>{/if}
          </a>
        {/each}
      {/if}
    </nav>

    {#if auth.me}
      <div class="border-t border-sidebar-border p-2">
        {#if !navCollapsed}
          <div class="truncate px-2 py-1 text-[11px] text-muted-foreground" title={auth.me.email ?? auth.me.user_id}>
            {auth.me.email ?? "Signed in"}
          </div>
        {/if}
        <button
          class="interactive flex w-full items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground {navCollapsed
            ? 'justify-center px-0'
            : 'px-3'}"
          title="Sign out"
          onclick={signOut}
        >
          <LogOut class="size-4 shrink-0" />
          {#if !navCollapsed}<span>Sign out</span>{/if}
        </button>
      </div>
    {/if}

    <div
      class="flex items-center border-t border-sidebar-border p-3 text-[11px] text-muted-foreground {navCollapsed
        ? 'justify-center'
        : 'gap-1.5'}"
    >
      {#if !navCollapsed}
        <span class="size-1.5 rounded-full bg-primary status-live"></span>
        <span class="font-mono">v0.3.3</span>
        <span class="font-mono" title="Deployed commit">· {COMMIT}</span>
        <button
          class="interactive ml-auto rounded-md p-1 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
          title="Collapse sidebar"
          aria-label="Collapse sidebar"
          onclick={toggleNav}
        >
          <PanelLeftClose class="size-4" />
        </button>
      {:else}
        <button
          class="interactive rounded-md p-1 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
          title="Expand sidebar"
          aria-label="Expand sidebar"
          onclick={toggleNav}
        >
          <PanelLeftOpen class="size-4" />
        </button>
      {/if}
    </div>
  </aside>

  <div class="flex min-w-0 flex-1 flex-col">
    {#if isAssuming()}
      <div class="flex items-center gap-2 border-b border-amber-500/30 bg-amber-500/10 px-4 py-1.5 text-xs text-amber-600 dark:text-amber-400">
        <span>Acting as another tenant — you have full access to this workspace, including live feeds.</span>
        <button class="interactive ml-auto inline-flex items-center gap-1 rounded-md px-2 py-0.5 font-medium hover:bg-amber-500/15" onclick={leaveTenant}>
          <Undo2 class="size-3.5" /> Exit
        </button>
      </div>
    {/if}
    <main class="flex-1 overflow-y-auto">
      {#key $page.url.pathname}
        {#if fullBleed}
          <div class="h-full overflow-hidden duration-200 animate-in fade-in">
            {@render children()}
          </div>
        {:else}
          <div
            class="mx-auto w-full max-w-6xl p-6 duration-300 animate-in fade-in slide-in-from-bottom-2"
          >
            {@render children()}
          </div>
        {/if}
      {/key}
    </main>
  </div>
</div>
{:else}
  {@render children()}
{/if}
{/if}

{#if showSplash}
  <Splash state={bootState} {leaving} />
{/if}

<Toaster />
