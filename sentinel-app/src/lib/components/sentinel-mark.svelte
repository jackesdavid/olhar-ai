<script lang="ts">
  // The Sentinel mark: a hexagonal dot-field (the logo) generated from a
  // pointy-top hex lattice with radial size + opacity falloff — dense/bright at
  // the center, sparse/dim at the edge. `animated` adds a slow radial "ping"
  // that ripples outward from the focal point (a sensor detecting).
  let {
    size = 28,
    animated = true,
    rings = 5,
    class: klass = "",
    style = "",
  }: {
    size?: number;
    animated?: boolean;
    rings?: number;
    class?: string;
    style?: string;
  } = $props();

  type Dot = { cx: number; cy: number; r: number; o: number; delay: number };

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
    return raw.map((p) => {
      const t = p.d / maxD; // 0 center … 1 edge
      return {
        cx: 50 + p.x * scale,
        cy: 50 + p.y * scale,
        r: 3.2 * (1 - Math.pow(t, 0.9)) + 0.45, // center ~3.65 → edge ~0.45
        o: 1 - 0.78 * t, // center 1 → edge ~0.22
        delay: -t * 1.1, // outward ripple (negative = staggered start)
      };
    });
  }

  const dots = build(rings);
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
  {#each dots as d, i (i)}
    <circle class="dot" cx={d.cx} cy={d.cy} r={d.r} style="--o:{d.o}; animation-delay:{d.delay}s" />
  {/each}
</svg>

<style>
  .dot {
    opacity: var(--o);
    transform-box: fill-box;
    transform-origin: center;
  }
  @media (prefers-reduced-motion: no-preference) {
    .is-animated .dot {
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
