<script lang="ts">
  // The canonical loading indicator: the Sentinel mark, pinging. Never a bare
  // spinner. The dot-field "scanning" reads as the system sensing.
  import SentinelMark from "./sentinel-mark.svelte";
  let { label = "", size = 40, class: klass = "" }: { label?: string; size?: number; class?: string } =
    $props();
</script>

<div class="flex flex-col items-center justify-center gap-3 py-10 {klass}">
  <SentinelMark {size} class="text-primary/80" />
  {#if label}
    <span class="text-sm text-muted-foreground">{label}</span>
  {/if}
</div>
