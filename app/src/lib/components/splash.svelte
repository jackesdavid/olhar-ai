<script lang="ts">
  // Full-screen boot splash: the large Sentinel mark, pinging, while the backend
  // connects to Postgres and applies migrations. Shown until `boot_status`
  // succeeds. Sits above everything on the same living sensor-field background.
  import SentinelMark from "./sentinel-mark.svelte";

  let { state = "connecting", leaving = false }: { state?: "connecting" | "error"; leaving?: boolean } =
    $props();

  // No nested ternaries in markup — resolve the copy in named helpers.
  function titleFor(s: "connecting" | "error"): string {
    if (s === "error") return "Waiting for database…";
    return "Initializing Sentinel…";
  }
  function detailFor(s: "connecting" | "error"): string {
    if (s === "error") return "Can't reach Postgres. Start it with “task db-up” — retrying…";
    return "Bringing services online";
  }

  const title = $derived(titleFor(state));
  const detail = $derived(detailFor(state));
</script>

<div
  class="fixed inset-0 z-50 flex flex-col items-center justify-center gap-8 bg-background text-foreground transition-opacity duration-500 {leaving
    ? 'pointer-events-none opacity-0'
    : 'opacity-100'}"
  role="status"
  aria-live="polite"
>
  <div class="glow rounded-full p-6">
    <SentinelMark size={176} animated class="text-primary" />
  </div>

  <div class="flex flex-col items-center gap-2 text-center">
    <span class="text-base font-medium tracking-tight">{title}</span>
    <span class="max-w-xs text-sm text-muted-foreground">{detail}</span>
  </div>
</div>
