<script lang="ts">
  // The Sentinel mark: a hexagonal dot-field (a sensor's field of awareness,
  // dense/bright at the centre, sparse/dim at the edge) with a slow radial "ping".
  //
  // Two forms share the same DNA:
  //   • default  — the bare pulsing field (loaders, splash, empty states, the "active" sensor).
  //   • boundary — the LOGO mark (#05a): the field contained within a rounded
  //     hexagonal perimeter + a solid focal core — "data in your control". Used by
  //     SentinelLogo, favicons and the OG image. See docs/ref/brand.md.
  let {
    size = 28,
    animated = true,
    rings = 5,
    speed = 3.4,
    boundary = false,
    class: klass = "",
    style = "",
  }: {
    size?: number;
    animated?: boolean;
    rings?: number;
    /** Ping cycle in seconds. Lower = livelier (inline busy); default = calm ambient. */
    speed?: number;
    /** Render the logo mark: rounded-hex perimeter + solid core around the field. */
    boundary?: boolean;
    class?: string;
    style?: string;
  } = $props();

  type Dot = { cx: number; cy: number; r: number; o: number; delay: number; solid?: boolean };

  // The rounded hexagonal perimeter (#05a), static — the "guarded perimeter".
  const HEX =
    "M 42.21 11.50 L 20.56 24.00 Q 12.76 28.50 12.76 37.50 L 12.76 62.50 Q 12.76 71.50 20.56 76.00 " +
    "L 42.21 88.50 Q 50.00 93.00 57.79 88.50 L 79.44 76.00 Q 87.24 71.50 87.24 62.50 L 87.24 37.50 " +
    "Q 87.24 28.50 79.44 24.00 L 57.79 11.50 Q 50.00 7.00 42.21 11.50 Z";

  function build(N: number): Dot[] {
    const raw: { x: number; y: number; d: number }[] = [];
    for (let q = -N; q <= N; q++) {
      for (let r = Math.max(-N, -q - N); r <= Math.min(N, -q + N); r++) {
        // pointy-top axial → pixel
        const x = Math.sqrt(3) * (q + r / 2);
        const y = 1.5 * r;
        raw.push({ x, y, d: Math.hypot(x, y) });
      }
    }
    const maxAbs = Math.max(...raw.map((p) => Math.max(Math.abs(p.x), Math.abs(p.y)))) || 1;
    const maxD = Math.max(...raw.map((p) => p.d)) || 1;
    const scale = 42 / maxAbs; // fit the field to ~84 of the 100 viewBox
    // In boundary mode the field is shrunk to sit inside the perimeter, with a solid core.
    const fs = boundary ? 0.5 : 1;
    return raw.map((p) => {
      const t = p.d / maxD; // 0 center … 1 edge
      const cx = 50 + p.x * scale * fs;
      const cy = 50 + p.y * scale * fs;
      if (boundary && t < 0.001) {
        return { cx, cy, r: 3.6, o: 1, delay: 0, solid: true }; // the focal core
      }
      const r = boundary
        ? (2.6 * (1 - Math.pow(t, 0.9)) + 0.5) * fs
        : 3.2 * (1 - Math.pow(t, 0.9)) + 0.45;
      return { cx, cy, r, o: 1 - 0.78 * t, delay: -t * 1.1 };
    });
  }

  const dots = $derived(build(boundary ? 3 : rings));
</script>

<svg
  viewBox="0 0 100 100"
  width={size}
  height={size}
  class="sentinel-mark {animated ? 'is-animated' : ''} {klass}"
  {style}
  fill="currentColor"
  role="img"
  aria-label="Sentinel"
>
  {#if boundary}
    <path d={HEX} fill="none" stroke="currentColor" stroke-width="3.4" />
  {/if}
  {#each dots as d, i (i)}
    <circle
      class="dot {d.solid ? 'is-core' : ''}"
      cx={d.cx}
      cy={d.cy}
      r={d.r}
      style="--o:{d.o}; animation-delay:{(d.delay * speed) / 3.4}s; animation-duration:{speed}s"
    />
  {/each}
</svg>

<style>
  .dot {
    opacity: var(--o);
    transform-box: fill-box;
    transform-origin: center;
  }
  @media (prefers-reduced-motion: no-preference) {
    .is-animated .dot:not(.is-core) {
      animation: mark-ping 3.4s ease-in-out infinite;
    }
  }
  @keyframes mark-ping {
    0%,
    100% {
      opacity: var(--o);
    }
    45% {
      opacity: calc(var(--o) * 0.32);
    }
  }
</style>
