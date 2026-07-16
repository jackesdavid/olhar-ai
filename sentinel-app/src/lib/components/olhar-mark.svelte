<script lang="ts">
  // The OlharAI mark: an eclipse-eye. A dotted orbit ring (40 dots), a disc eclipsed by an
  // offset circle to cut a crescent, a pupil, and a moon riding the ring.
  //
  // Two independent motions, deliberately:
  //
  //   • The moon revolves the ring, and the ring's 40 dots drift the opposite way at a quarter
  //     speed — the satellite, calm and mechanical.
  //   • The eye behaves like an EYE, not a planet: it holds still, gazes in saccades (quick
  //     shifts, long fixations — how eyes actually move; a smoothly drifting pupil reads as
  //     drunk), and blinks now and then.
  //
  // Note for anyone reading the art: in the supplied SVGs the eclipse, pupil and moon all sit on
  // one radial line at exactly 45°, which reads as "the moon casts the shadow". We deliberately
  // do NOT animate that relationship — revolving the eclipse with the moon turned the eye into a
  // planet. So the 45° alignment is now the mark's resting composition rather than a causal
  // mechanism. That's an owner decision, not an oversight.
  //
  // Ported from `assets/olhar-ai-mark-*.svg` into a component (rebrand card #76, Phase 2a): the
  // static files can't give us `currentColor` theming (2d), per-instance mask ids (2e), or any
  // motion at all — the supplied `mark-animated.svg` declares .orbit/.moon/.pupil and defines no
  // animation (2c).
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
    /** Animate the moon, the gaze and the blink. Honours `prefers-reduced-motion` either way. */
    animated?: boolean;
    /** Seconds for one moon revolution. Higher = calmer; this is a logo, not a spinner. */
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

  // The art's geometry, verbatim.
  const R_ORBIT = 50;
  const R_DISC = 31;
  const R_ECLIPSE = 27;
  const ECL = { cx: 71, cy: 49 }; // the eye opening — also the pupil's home
  const MOON = { cx: 95.4, cy: 24.6 };

  const ink = $derived(tile ? "var(--background)" : "currentColor");
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
  <!-- The eye opening is a hole in the disc, so the "eyelid" is this circle flattening: scaleY→0
       fills the crescent back in and the eye reads as shut. The pupil below blinks in lockstep. -->
  <mask id={maskId}>
    <rect width="120" height="120" fill="white" />
    <g class="lid">
      <circle cx={ECL.cx} cy={ECL.cy} r={R_ECLIPSE} fill="black" />
    </g>
  </mask>

  {#if tile}
    <rect width="120" height="120" rx="14" fill="currentColor" />
  {/if}

  <circle
    class="orbit"
    cx="60"
    cy="60"
    r={R_ORBIT}
    stroke={ink}
    stroke-width="2"
    stroke-dasharray="0.1 7.754"
    stroke-linecap="round"
    opacity="0.3"
  />

  <circle cx="60" cy="60" r={R_DISC} fill={ink} mask="url(#{maskId})" />

  <!-- Nested so blink and gaze can both use `transform` without fighting: the lid squashes the
       group, the pupil shifts inside it. -->
  <g class="lid">
    <circle class="gaze" cx={ECL.cx} cy={ECL.cy} r="6" fill={ink} />
  </g>

  <circle class="moon" cx={MOON.cx} cy={MOON.cy} r="4" fill={ink} />
</svg>

<style>
  /* The satellite turns about the mark's centre; the eye's parts move about the eye. Getting
     these origins wrong is what makes an eye look like a planet. */
  .moon,
  .orbit {
    transform-box: view-box;
    transform-origin: 60px 60px;
  }
  .lid,
  .gaze {
    transform-box: view-box;
    transform-origin: 71px 49px;
  }

  /* A logo that animates through a user's reduced-motion preference is a logo that ignores them.
     The static state is the supplied art exactly, so nothing is lost. */
  @media (prefers-reduced-motion: no-preference) {
    .is-animated .moon {
      animation: olhar-revolve var(--rev, 12s) linear infinite;
    }
    .is-animated .orbit {
      animation: olhar-drift calc(var(--rev, 12s) * 4) linear infinite;
    }
    /* Prime numbers-ish: 7s and 11s drift out of phase, so the mark never loops visibly. */
    .is-animated .lid {
      animation: olhar-blink 7s ease-in-out infinite;
    }
    .is-animated .gaze {
      animation: olhar-gaze 11s ease-in-out infinite;
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

  /* One blink per cycle, ~220ms of a 7s loop — closing faster than opening, as real lids do. */
  @keyframes olhar-blink {
    0%,
    93%,
    100% {
      transform: scaleY(1);
    }
    95.5% {
      transform: scaleY(0.04);
    }
    98% {
      transform: scaleY(1);
    }
  }

  /* Saccades: the shift takes ~3% of the loop, then the eye FIXATES. Eyes jump and hold; they
     never glide. The travel is small (≤5px in a 27px opening) — a glance, not a search. */
  @keyframes olhar-gaze {
    0%,
    24% {
      transform: translate(0, 0);
    }
    27%,
    46% {
      transform: translate(-4.5px, 1.5px);
    }
    49%,
    68% {
      transform: translate(3.5px, -3px);
    }
    71%,
    88% {
      transform: translate(1px, 3.5px);
    }
    91%,
    100% {
      transform: translate(0, 0);
    }
  }
</style>
