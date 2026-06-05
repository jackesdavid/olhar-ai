<script lang="ts">
  // A status dot. "online" pings (the signature pulse); others sit calm.
  let {
    state = "online",
    class: klass = "",
  }: { state?: "online" | "offline" | "warn" | "danger"; class?: string } = $props();

  const color: Record<string, string> = {
    online: "bg-primary",
    offline: "bg-muted-foreground/40",
    warn: "bg-amber-400",
    danger: "bg-red-400",
  };
</script>

<span
  class="inline-block size-2 shrink-0 rounded-full {color[state]} {state === 'online'
    ? 'status-live'
    : ''} {klass}"
></span>
