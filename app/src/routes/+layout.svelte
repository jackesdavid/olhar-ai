<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { navItems } from "$lib/nav";
  import { Toaster } from "$lib/components/ui/sonner";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";

  let { children } = $props();

  function isActive(href: string): boolean {
    const path = $page.url.pathname;
    if (href === "/") return path === "/";
    return path === href || path.startsWith(href + "/");
  }
</script>

<div class="flex h-screen w-screen overflow-hidden bg-background text-foreground">
  <aside
    class="flex w-60 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground"
  >
    <div class="flex h-14 items-center gap-2 border-b border-sidebar-border px-4">
      <div
        class="flex size-8 items-center justify-center rounded-md bg-primary text-primary-foreground"
      >
        <ShieldCheck class="size-5" />
      </div>
      <div class="flex flex-col leading-tight">
        <span class="text-sm font-semibold tracking-tight">Sentinel</span>
        <span class="text-[10px] text-muted-foreground">Monitoring &amp; Automation</span>
      </div>
    </div>

    <nav class="flex-1 space-y-1 overflow-y-auto p-2">
      {#each navItems as item (item.href)}
        {@const Icon = item.icon}
        <a
          href={item.href}
          data-active={isActive(item.href)}
          class="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground"
        >
          <Icon class="size-4 shrink-0" />
          <span>{item.label}</span>
        </a>
      {/each}
    </nav>

    <div class="border-t border-sidebar-border p-3 text-[11px] text-muted-foreground">
      <span class="font-mono">v0.1.0</span> · M0 foundation
    </div>
  </aside>

  <div class="flex min-w-0 flex-1 flex-col">
    <main class="flex-1 overflow-y-auto">
      <div class="mx-auto w-full max-w-6xl p-6">
        {@render children()}
      </div>
    </main>
  </div>
</div>

<Toaster />
