<script lang="ts">
  // The one page header: leading icon-chip + title + description on the left,
  // optional actions on the right. Codifies the canonical title type scale and the
  // icon treatment so every page reads identically — don't hand-roll page headers.
  import type { Component, Snippet } from "svelte";
  import IconChip from "./icon-chip.svelte";
  let {
    title,
    description = "",
    icon,
    actions,
  }: {
    title: string;
    description?: string;
    /** Lucide icon component — rendered inside the chip. Usually the page's nav icon. */
    icon?: Component;
    /** Right-aligned controls (buttons, a status pill, etc.). */
    actions?: Snippet;
  } = $props();
</script>

<div class="flex items-center justify-between gap-3">
  <div class="flex min-w-0 items-center gap-3">
    {#if icon}
      {@const Icon = icon}
      <IconChip><Icon class="size-5" /></IconChip>
    {/if}
    <div class="min-w-0">
      <h1 class="truncate text-2xl font-semibold tracking-tight">{title}</h1>
      {#if description}
        <p class="text-sm text-muted-foreground">{description}</p>
      {/if}
    </div>
  </div>
  {#if actions}
    <div class="flex shrink-0 items-center gap-2">{@render actions()}</div>
  {/if}
</div>
