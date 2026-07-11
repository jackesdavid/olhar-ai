<script lang="ts">
  // The Sentinel logo lockup: the dot-field mark + the "Sentinel" wordmark
  // (Sora, sentence-case — the locked brand type). Horizontal by default; set
  // `stacked` for square contexts. `wordmark={false}` renders the mark alone
  // (e.g. a collapsed sidebar). The wordmark uses --font-brand (Sora), scoped
  // to the logo only — it does NOT change the app's UI font.
  import SentinelMark from "./sentinel-mark.svelte";

  let {
    size = 30,
    wordmark = true,
    stacked = false,
    animated = false,
    subtitle = "",
    class: klass = "",
    markClass = "text-primary",
  }: {
    /** Mark height in px; the wordmark is cap-height-matched to it. */
    size?: number;
    /** Show the "Sentinel" wordmark. False = mark only. */
    wordmark?: boolean;
    /** Stack the wordmark under the mark (square contexts). */
    stacked?: boolean;
    /** Ping the mark. Off by default — a logo shouldn't animate constantly. */
    animated?: boolean;
    /** Optional tagline under the wordmark (e.g. "Monitoring & Automation"). */
    subtitle?: string;
    class?: string;
    markClass?: string;
  } = $props();

  // Cap-height match + clear-space, derived from the mark size (the study's ratios).
  const gap = $derived(Math.round(size * (stacked ? 0.24 : 0.36)));
  const wm = $derived(Math.round(size * (stacked ? 0.5 : 0.66)));
</script>

<span
  class="sentinel-logo {stacked ? 'is-stacked' : ''} {klass}"
  style="gap:{gap}px"
>
  <SentinelMark
    {size}
    {animated}
    rings={4}
    class="shrink-0 {markClass}"
    style="filter: drop-shadow(var(--brand-glow))"
  />
  {#if wordmark}
    <span class="lockup-text">
      <span class="wordmark" style="font-size:{wm}px">Sentinel</span>
      {#if subtitle}<span class="subtitle">{subtitle}</span>{/if}
    </span>
  {/if}
</span>

<style>
  .sentinel-logo {
    display: inline-flex;
    align-items: center;
  }
  .sentinel-logo.is-stacked {
    flex-direction: column;
    text-align: center;
  }
  .lockup-text {
    display: flex;
    flex-direction: column;
    line-height: 1;
  }
  .wordmark {
    font-family: var(--font-brand);
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--foreground);
  }
  .subtitle {
    font-size: 0.5em;
    margin-top: 0.55em;
    letter-spacing: 0.01em;
    color: var(--muted-foreground);
  }
</style>
