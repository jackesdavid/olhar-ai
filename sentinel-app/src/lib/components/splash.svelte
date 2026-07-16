<script lang="ts">
  // Full-screen boot splash: the large OlharAI mark, its moon revolving, while the backend
  // connects to Postgres and applies migrations. Shown until `boot_status` succeeds. Sits above
  // everything on the same living sensor-field background.
  //
  // The mark enters rather than appearing: the splash is the first thing anyone sees, and a logo
  // that fades up and settles reads as the product waking up — which is literally what's
  // happening behind it. The revolution then loops for as long as the wait lasts, so a slow boot
  // looks like patience instead of a freeze.
  import OlharMark from "./olhar-mark.svelte";

  let { state = "connecting", leaving = false }: { state?: "connecting" | "error"; leaving?: boolean } =
    $props();

  // No nested ternaries in markup — resolve the copy in named helpers.
  function titleFor(s: "connecting" | "error"): string {
    if (s === "error") return "Waiting for database…";
    return "Initializing OlharAI…";
  }
  function detailFor(s: "connecting" | "error"): string {
    if (s === "error") return "Can't reach Postgres. Start it with “task db-up” — retrying…";
    return "Bringing services online";
  }
  // The wait slows the revolution — calm while it's working, and a visible change of state when
  // it isn't. Nobody should have to read the copy to notice something went wrong.
  function speedFor(s: "connecting" | "error"): number {
    if (s === "error") return 24;
    return 9;
  }

  const title = $derived(titleFor(state));
  const detail = $derived(detailFor(state));
  const speed = $derived(speedFor(state));
</script>

<div
  class="fixed inset-0 z-50 flex flex-col items-center justify-center gap-8 bg-background text-foreground transition-opacity duration-500 {leaving
    ? 'pointer-events-none opacity-0'
    : 'opacity-100'}"
  role="status"
  aria-live="polite"
>
  <div class="glow enter rounded-full p-6">
    <OlharMark size={176} animated {speed} class="text-primary" label="" />
  </div>

  <div class="enter enter-copy flex flex-col items-center gap-2 text-center">
    <span class="text-base font-medium tracking-tight">{title}</span>
    <span class="max-w-xs text-sm text-muted-foreground">{detail}</span>
  </div>
</div>

<style>
  /* The mark settles in first, the copy follows a beat later — the eye leads, the words explain.
     Reduced motion gets the same composition with no travel. */
  .enter {
    animation: splash-enter 900ms cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  .enter-copy {
    animation-delay: 260ms;
  }

  @keyframes splash-enter {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.94);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .enter {
      animation: splash-fade 300ms ease-out both;
    }
    @keyframes splash-fade {
      from {
        opacity: 0;
      }
      to {
        opacity: 1;
      }
    }
  }
</style>
