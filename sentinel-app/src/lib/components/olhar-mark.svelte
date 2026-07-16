<script lang="ts">
  // The OlharAI mark: an eclipse-eye. A dotted orbit ring (40 dots), a disc eclipsed by an
  // offset circle to cut a crescent, a pupil, and a moon riding the ring.
  //
  // The motion comes from the geometry, not from taste. In the supplied art the eclipse, the
  // pupil and the moon all sit at *exactly* 45° from centre (radius 15.56 / 15.56 / 50.06) —
  // the moon and the shadow it casts are on one radial line. So the mark animates the only way
  // it can honestly move: the moon revolves, and the eclipse + pupil revolve with it. The
  // crescent turns and the eye looks wherever the moon is. Nothing here is arbitrary; change the
  // angles and you break the eclipse.
  //
  // Ported from `assets/olhar-ai-mark-*.svg` into a procedural component (rebrand card #76,
  // Phase 2a) because the static files can't serve what the app needs: `currentColor` so one
  // mark themes light/dark (2d), per-instance mask ids so two marks on one page don't collide
  // (2e), and real motion — the supplied `mark-animated.svg` declares .orbit/.moon/.pupil and
  // defines no animation at all (2c).
  let {
    size = 28,
    animated = true,
    speed = 12,
    tile = false,
    class: klass = "",
    style = "",
    label = "OlharAI",
  }: {
    size?: number;
    /** Revolve the moon + eclipse. Honours `prefers-reduced-motion` either way. */
    animated?: boolean;
    /** Seconds for one full revolution. Higher = calmer; this is a logo, not a spinner. */
    speed?: number;
    /** Draw the app-icon's ink tile behind the mark (the `boundary` role of the old mark). */
    tile?: boolean;
    class?: string;
    style?: string;
    /** Accessible name. Set "" for a decorative instance beside real text. */
    label?: string;
  } = $props();

  // Every supplied SVG hardcodes `id="ecl"`, so two inlined marks would make the second one's
  // disc reference the first one's mask. Ids must be per-instance.
  const uid = $props.id();
  const maskId = `olhar-ecl-${uid}`;

  // The art's geometry, verbatim — see the module comment before touching any of it.
  const R_ORBIT = 50;
  const R_DISC = 31;
  const R_ECLIPSE = 27;
  const ECL = { cx: 71, cy: 49 }; // 45°, r=15.56
  const MOON = { cx: 95.4, cy: 24.6 }; // 45°, r=50.06 — same radial line
</script>

<svg
  viewBox="0 0 120 120"
  width={size}
  height={size}
  class="olhar-mark {animated ? 'is-animated' : ''} {klass}"
  {style}
  fill="none"
  role={label ? "img" : "presentation"}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : "true"}
  style:--rev="{speed}s"
>
  <mask id={maskId}>
    <rect width="120" height="120" fill="white" />
    <circle class="revolve" cx={ECL.cx} cy={ECL.cy} r={R_ECLIPSE} fill="black" />
  </mask>

  {#if tile}
    <rect width="120" height="120" rx="14" fill="currentColor" />
  {/if}

  <!-- The orbit: 40 dots (dash pitch 7.854 over a 314.16 circumference). It drifts the opposite
       way to the moon, slowly — depth, not motion you'd notice looking straight at it. -->
  <circle
    class="orbit"
    cx="60"
    cy="60"
    r={R_ORBIT}
    stroke={tile ? "var(--background)" : "currentColor"}
    stroke-width="2"
    stroke-dasharray="0.1 7.754"
    stroke-linecap="round"
    opacity="0.3"
  />

  <circle
    cx="60"
    cy="60"
    r={R_DISC}
    fill={tile ? "var(--background)" : "currentColor"}
    mask="url(#{maskId})"
  />
  <circle class="revolve" cx={ECL.cx} cy={ECL.cy} r="6" fill={tile ? "var(--background)" : "currentColor"} />
  <circle class="revolve" cx={MOON.cx} cy={MOON.cy} r="4" fill={tile ? "var(--background)" : "currentColor"} />
</svg>

<style>
  /* `view-box` + an explicit origin: the parts revolve around the mark's centre, not their own
     bounding boxes — that distinction is the whole animation. */
  .revolve,
  .orbit {
    transform-box: view-box;
    transform-origin: 60px 60px;
  }

  /* A logo that animates through a user's reduced-motion preference is a logo that ignores them.
     The static state is the supplied art exactly, so nothing is lost. */
  @media (prefers-reduced-motion: no-preference) {
    .is-animated .revolve {
      animation: olhar-revolve var(--rev, 12s) linear infinite;
    }
    .is-animated .orbit {
      animation: olhar-drift calc(var(--rev, 12s) * 4) linear infinite;
    }
  }

  @keyframes olhar-revolve {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes olhar-drift {
    to {
      transform: rotate(-360deg);
    }
  }
</style>
