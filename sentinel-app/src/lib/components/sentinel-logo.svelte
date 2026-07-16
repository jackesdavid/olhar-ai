<script lang="ts">
  // The OlharAI logo lockup: the eclipse-eye mark + the "OLHAR AI" wordmark. Horizontal by
  // default; set `stacked` for square contexts. `wordmark={false}` renders the mark alone (e.g. a
  // collapsed sidebar). The wordmark uses --font-brand (Sora), scoped to the logo only — it does
  // NOT change the app's UI font.
  //
  // Per `assets/BRAND.md` § Typography: **Sora 600, UPPERCASE, letter-spacing 0.16em** (0.18em at
  // display sizes), and the lockup sets the mark at ~1.6x the cap height with a gap of ~0.65x cap
  // height. The old lockup was sentence-case at -0.02em tracking — the inverse of this brand, so
  // none of those numbers survive.
  //
  // The file keeps its `sentinel-*` name on purpose: the rebrand renames what users see, not the
  // codebase (see docs/plans/olharai-rebrand.md).
  import OlharMark from "./olhar-mark.svelte";

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

  // BRAND.md gives the lockup in CAP HEIGHTS, not font sizes: mark = 1.6x cap, gap = 0.65x cap.
  // Sora's cap height is ~0.7em, so font-size = cap / 0.7. Verify against the font's real metrics
  // if the lockup ever looks off — this constant is the one soft number here.
  const SORA_CAP = 0.7;
  const cap = $derived(size / 1.6);
  const wm = $derived(Math.round((cap / SORA_CAP) * (stacked ? 0.82 : 1)));
  const gap = $derived(Math.round(cap * (stacked ? 0.45 : 0.65)));
  // 0.18em at display sizes, 0.16em otherwise (BRAND.md § Typography).
  const tracking = $derived(size >= 48 ? "0.18em" : "0.16em");
</script>

<span
  class="sentinel-logo {stacked ? 'is-stacked' : ''} {klass}"
  style="gap:{gap}px"
>
  <OlharMark
    {size}
    {animated}
    label=""
    class="shrink-0 {markClass}"
    style="filter: drop-shadow(var(--brand-glow))"
  />
  {#if wordmark}
    <span class="lockup-text">
      <span class="wordmark" style="font-size:{wm}px; letter-spacing:{tracking}">OLHAR AI</span>
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
    /* Uppercase is the wordmark's styling, not the product's name — BRAND.md sets the mark as
       "OLHAR AI" while copy uses the canonical name. Tracking is set inline (size-dependent). */
    text-transform: uppercase;
    color: var(--foreground);
    white-space: nowrap;
  }
  .subtitle {
    font-size: 0.5em;
    margin-top: 0.55em;
    letter-spacing: 0.01em;
    color: var(--muted-foreground);
  }
</style>
