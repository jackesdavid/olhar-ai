<script lang="ts">
  // Icon library for the Marker tool. Pick an icon, then click the floor to drop it.
  import { MARKER_ICONS } from "$lib/map/icons";
  import type { ToolState } from "$lib/map/tools.svelte";

  let { tools }: { tools: ToolState } = $props();

  function choose(key: string) {
    tools.markerIcon = key;
    tools.setMode("marker");
  }

  function tileClass(key: string): string {
    const base =
      "flex aspect-square items-center justify-center rounded-md border p-2 text-primary transition-colors [&_svg]:size-7";
    const on = "border-primary/60 bg-primary/10";
    const off = "border-border hover:bg-hover";
    return `${base} ${tools.markerIcon === key ? on : off}`;
  }
</script>

<div class="grid grid-cols-4 gap-1.5">
  {#each MARKER_ICONS as icon (icon.key)}
    <button type="button" class={tileClass(icon.key)} title={icon.label} onclick={() => choose(icon.key)}>
      {#if icon.svg}
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        {@html icon.svg}
      {:else if icon.src}
        <img src={icon.src} class="size-full object-contain opacity-70" alt={icon.label} />
      {/if}
    </button>
  {/each}
</div>
