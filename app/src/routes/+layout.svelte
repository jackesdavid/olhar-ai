<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import { navItems } from "$lib/nav";
  import { Toaster } from "$lib/components/ui/sonner";
  import SentinelMark from "$lib/components/sentinel-mark.svelte";
  import SensorField from "$lib/components/sensor-field.svelte";
  import Splash from "$lib/components/splash.svelte";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";

  let { children } = $props();

  // Boot gate: the app requires Postgres (no JSON fallback), so we hold the splash
  // until `boot_status` confirms the DB is connected + migrated. The app mounts
  // only after that, so first-load data fetches see the real device list.
  let ready = $state(false); // app content mounted
  let showSplash = $state(true); // splash present
  let leaving = $state(false); // splash fading out
  let bootState = $state<"connecting" | "error">("connecting");

  const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

  async function bootSequence() {
    const started = Date.now();
    for (;;) {
      try {
        await invoke("boot_status");
        break;
      } catch {
        bootState = "error";
        await delay(1500); // keep retrying until Postgres is up + migrated
      }
    }
    const elapsed = Date.now() - started;
    if (elapsed < 600) await delay(600 - elapsed); // let the animation read
    ready = true; // mount the app behind the splash…
    leaving = true; // …and crossfade the splash out
    await delay(550);
    showSplash = false;
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
  onMount(() => {
    if (localStorage.getItem("sentinel.navCollapsed") === "true") navCollapsed = true;
    bootSequence();
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
</script>

<SensorField />

{#if ready}
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
      {#each navItems as item (item.href)}
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
    </nav>

    <div
      class="flex items-center border-t border-sidebar-border p-3 text-[11px] text-muted-foreground {navCollapsed
        ? 'justify-center'
        : 'gap-1.5'}"
    >
      {#if !navCollapsed}
        <span class="size-1.5 rounded-full bg-primary status-live"></span>
        <span class="font-mono">v0.1.0</span>
        <span>· ready</span>
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
{/if}

{#if showSplash}
  <Splash state={bootState} {leaving} />
{/if}

<Toaster />
