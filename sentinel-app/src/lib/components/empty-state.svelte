<script lang="ts">
  // Empty state: the mark as a faint watermark + one helpful line + an optional
  // action (passed as children). Never a bare "nothing here" message.
  import type { Snippet } from "svelte";
  import OlharMark from "./olhar-mark.svelte";
  let {
    title,
    description = "",
    children,
  }: { title: string; description?: string; children?: Snippet } = $props();
</script>

<div class="flex flex-col items-center justify-center gap-4 py-14 text-center">
  <OlharMark size={60} animated={false} class="text-primary/25" label="" />
  <div class="space-y-1">
    <p class="text-sm font-medium">{title}</p>
    {#if description}
      <p class="mx-auto max-w-sm text-sm text-muted-foreground">{description}</p>
    {/if}
  </div>
  {#if children}
    <div class="mt-1">{@render children()}</div>
  {/if}
</div>
