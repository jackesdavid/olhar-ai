<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { navItems } from "$lib/nav";
  import { Toaster } from "$lib/components/ui/sonner";
  import SentinelMark from "$lib/components/sentinel-mark.svelte";
  import SensorField from "$lib/components/sensor-field.svelte";

  let { children } = $props();

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

<div class="flex h-screen w-screen overflow-hidden text-foreground">
  <aside
    class="flex w-60 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground"
  >
    <div class="flex h-14 items-center gap-2.5 border-b border-sidebar-border px-4">
      <SentinelMark
        size={32}
        class="text-primary drop-shadow-[0_0_10px_oklch(0.7_0.16_162/0.45)]"
      />
      <div class="flex flex-col leading-tight">
        <span class="text-sm font-semibold tracking-tight">Sentinel</span>
        <span class="text-[10px] text-muted-foreground">Monitoring &amp; Automation</span>
      </div>
    </div>

    <nav class="flex-1 space-y-0.5 overflow-y-auto p-2">
      {#each navItems as item (item.href)}
        {@const Icon = item.icon}
        {@const active = isActive(item.href)}
        <a
          href={item.href}
          data-active={active}
          class="group relative flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground"
        >
          <span
            class="absolute left-0 top-1/2 h-5 -translate-y-1/2 rounded-r-full bg-primary transition-all duration-200 {active
              ? 'w-1 opacity-100'
              : 'w-0 opacity-0'}"
          ></span>
          <Icon class="size-4 shrink-0 transition-colors group-data-[active=true]:text-primary" />
          <span>{item.label}</span>
        </a>
      {/each}
    </nav>

    <div
      class="flex items-center gap-1.5 border-t border-sidebar-border p-3 text-[11px] text-muted-foreground"
    >
      <span class="size-1.5 rounded-full bg-primary status-live"></span>
      <span class="font-mono">v0.1.0</span>
      <span>· ready</span>
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

<Toaster />
