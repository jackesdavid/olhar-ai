<script lang="ts">
  // The canonical loading indicator: the OlharAI mark, alive. Never a bare spinner — the eye
  // breathing and glancing reads as the system looking, which is the whole product.
  import OlharMark from "./olhar-mark.svelte";
  let { label = "", size = 40, class: klass = "" }: { label?: string; size?: number; class?: string } =
    $props();
</script>

<div class="flex flex-col items-center justify-center gap-3 py-10 {klass}">
  <OlharMark {size} class="text-primary/80" label="" />
  {#if label}
    <span class="text-sm text-muted-foreground">{label}</span>
  {/if}
</div>
