<script lang="ts">
  // The OlharAI mark: "Eclipse in orbit" — an eclipsed disc (a gaze emerging from shadow) with a
  // pupil, inside a dotted orbit ring with a small moon. Space + vision.
  //
  // **`assets/BRAND.md` is the spec. It wins.** Geometry, timings and easings below are its
  // numbers, not preferences — the dot pitch divides the circumference exactly (no seam), and the
  // glance offsets are the designer's. Recompute the dasharray if you ever change `r`.
  //
  // The motion (BRAND.md § Animation):
  //   • Orbit ring — 360° / 90s linear.
  //   • Moon — 360° / 24s linear, drawn BEFORE the disc so it passes *behind* it.
  //   • Pupil breathe — scale 1→1.12→1 / 3.4s.
  //   • Moving eye — the crescent-mask group AND the pupil group translate *together*: glance
  //     left (-19,4), down (-8,16), return. 9s with holds. Because the disc stays put while the
  //     opening travels inside it, the crescent thickens and thins as the eye looks around —
  //     that's the form working, and it's why both groups must share one animation.
  //   • `prefers-reduced-motion` → none.
  //
  // `pace` divides every duration. It exists ONLY because this form doubles as the app's inline
  // "working" indicator (`busy`, 22 call sites), where a 24s moon reads as frozen. BRAND.md's
  // timings govern the mark as a LOGO — splash, lockup, empty states — and those all use pace=1.
  // Don't reach for it to make a logo livelier.
  //
  // Deviation, on purpose: the **blink** is NOT in BRAND.md — it's ours, kept because the owner
  // asked for the centre to behave like an eye and endorsed it. The lid works *because* the
  // opening is a hole in the disc: flattening it (scaleY→0.04) fills the crescent back in, so the
  // eye genuinely reads as shut. Drop `blink` to get the spec exactly.
  //
  // Ported from the static art into a component (rebrand #76, Phase 2a): the files can't give us
  // `currentColor` theming (2d), per-instance mask ids (2e), the <40px variant, or any motion —
  // `mark-animated.svg` declares .orbit/.moon/.pupil and defines none (2c).
  let {
    size = 28,
    animated = true,
    blink = true,
    tile = false,
    minimal = undefined,
    pace = 1,
    class: klass = "",
    style = "",
    label = "Filma AI",
  }: {
    size?: number;
    /** Animate orbit, moon, breathe and glance. Honours `prefers-reduced-motion` either way. */
    animated?: boolean;
    /** The one motion not in BRAND.md. Off = the spec exactly. */
    blink?: boolean;
    /** Draw the app-icon's ink tile behind the mark (BRAND.md: rx 14/120). */
    tile?: boolean;
    /** Crop to the eye and drop the orbit + moon. Auto below 24px, where they're sub-pixel. */
    minimal?: boolean;
    /** Multiply the motion's tempo. 1 = BRAND.md. Only for UI affordances — see the note above. */
    pace?: number;
    class?: string;
    style?: string;
    /** Accessible name. Set "" for a decorative instance beside real text. */
    label?: string;
  } = $props();

  // Every supplied SVG hardcodes `id="ecl"`, so two inlined marks would make the second one's
  // disc reference the first one's mask. Ids must be per-instance.
  const uid = $props.id();
  const maskId = `olhar-ecl-${uid}`;

  const R_ORBIT = 50;
  const R_DISC = 31;
  const R_ECLIPSE = 27;
  const ECL = { cx: 71, cy: 49 }; // the opening — and the pupil's home
  const MOON = { cx: 95.4, cy: 24.6 }; // on the orbit at 45°

  // BRAND.md § The mark: under 40px the ring goes heavier and sparser, or it mushes. This is why
  // the supplied lockups carry different ring values from the standalone mark — one mark, two
  // size variants, not two marks.
  const sm = $derived(size < 40);
  const ring = $derived(
    sm
      ? { width: 3, dash: "0.1 9.717", opacity: 0.35, moon: 5 } // 32 dots
      : { width: 2, dash: "0.1 7.754", opacity: 0.3, moon: 4 }, // 40 dots
  );

  // Below ~24px the orbit's 0.4px stroke and the 1.3px moon are sub-pixel — they don't shrink,
  // they smear. So the mark sheds them and crops to the eye, which is the part that carries the
  // identity anyway. BRAND.md's small variant stops at 40px; this is the tier below it, and the
  // same reduction the favicon will need at 16px.
  const isMin = $derived(minimal ?? size < 24);
  // The disc spans 29–91 of the 120 box, so at full viewBox it's only ~half the frame. Cropping
  // to it lets the eye fill a 16px button the way a 176px splash mark fills its space.
  const box = $derived(isMin ? "25 25 70 70" : "0 0 120 120");

  const ink = $derived(tile ? "var(--background)" : "currentColor");
</script>

<svg
  viewBox={box}
  width={size}
  height={size}
  class="olhar-mark {animated ? 'is-animated' : ''} {blink ? 'can-blink' : ''} {klass}"
  {style}
  fill="none"
  role={label ? "img" : "presentation"}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : "true"}
  style:--pace={pace}
>
  <!-- The opening carries `glance` exactly as the pupil does (same keyframes, same duration), so
       the negative space and the pupil travel as one eye. -->
  <mask id={maskId}>
    <rect width="120" height="120" fill="white" />
    <g class="lid">
      <g class="glance">
        <circle cx={ECL.cx} cy={ECL.cy} r={R_ECLIPSE} fill="black" />
      </g>
    </g>
  </mask>

  {#if tile}
    <rect width="120" height="120" rx="14" fill="currentColor" />
  {/if}

  {#if !isMin}
    <circle
      class="orbit"
      cx="60"
      cy="60"
      r={R_ORBIT}
      stroke={ink}
      stroke-width={ring.width}
      stroke-dasharray={ring.dash}
      stroke-linecap="round"
      opacity={ring.opacity}
    />

    <!-- Before the disc, per BRAND.md — the moon rides the ring and passes behind the eye. -->
    <circle class="moon" cx={MOON.cx} cy={MOON.cy} r={ring.moon} fill={ink} />
  {/if}

  <circle cx="60" cy="60" r={R_DISC} fill={ink} mask="url(#{maskId})" />

  <!-- Three nested groups so lid/glance/breathe can each drive `transform` without fighting. -->
  <g class="lid">
    <g class="glance">
      <circle class="breathe" cx={ECL.cx} cy={ECL.cy} r="6" fill={ink} />
    </g>
  </g>
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
  .glance,
  .breathe {
    transform-box: view-box;
    transform-origin: 71px 49px;
  }

  /* A logo that animates through a user's reduced-motion preference is a logo that ignores them.
     The static state is the supplied art exactly, so nothing is lost. */
  @media (prefers-reduced-motion: no-preference) {
    .is-animated .orbit {
      animation: olhar-orbit calc(90s / var(--pace, 1)) linear infinite;
    }
    .is-animated .moon {
      animation: olhar-moon calc(24s / var(--pace, 1)) linear infinite;
    }
    .is-animated .breathe {
      animation: olhar-breathe calc(3.4s / var(--pace, 1)) cubic-bezier(0.4, 0, 0.6, 1) infinite;
    }
    .is-animated .glance {
      animation: olhar-glance calc(9s / var(--pace, 1)) cubic-bezier(0.4, 0, 0.2, 1) infinite;
    }
    /* Ours, not the spec's. 7s against the glance's 9s so they drift out of phase. */
    .is-animated.can-blink .lid {
      animation: olhar-blink calc(7s / var(--pace, 1)) ease-in-out infinite;
    }
  }

  @keyframes olhar-orbit {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes olhar-moon {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes olhar-breathe {
    0%,
    100% {
      transform: scale(1);
    }
    50% {
      transform: scale(1.12);
    }
  }

  /* Glance left, glance down, return — each move followed by a hold. Eyes jump and fixate; they
     never glide. */
  @keyframes olhar-glance {
    0%,
    22% {
      transform: translate(0, 0);
    }
    30%,
    48% {
      transform: translate(-19px, 4px);
    }
    56%,
    74% {
      transform: translate(-8px, 16px);
    }
    82%,
    100% {
      transform: translate(0, 0);
    }
  }

  /* One blink per cycle, ~220ms of 7s — closing faster than opening, as real lids do. */
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
</style>
