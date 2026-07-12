<script lang="ts">
  // A privacy-safe "avatar" for an enrolled face: a deterministic dot-field derived
  // from the embedding vector — never the photo. Same vector → same glyph, so each
  // enrollment reads as a recognisable-yet-abstract fingerprint (the product's
  // dot-field identity). No PII leaves the embedding.
  let {
    embedding,
    size = 96,
    dots = 200,
  }: { embedding: number[]; size?: number; dots?: number } = $props();

  type Dot = { x: number; y: number; r: number; o: number };

  // Min-max normalise an array to 0..1 (flat array → all 0.5, a centred dot).
  function norm(arr: number[]): number[] {
    let min = Infinity;
    let max = -Infinity;
    for (const a of arr) {
      if (a < min) min = a;
      if (a > max) max = a;
    }
    const span = max - min || 1;
    return arr.map((a) => (a - min) / span);
  }

  const points = $derived.by(() => {
    const v = embedding ?? [];
    if (v.length < 4) return [] as Dot[];
    // Split the vector into X and Y halves → one dot per paired component.
    const n = Math.min(dots, Math.floor(v.length / 2));
    const xs = v.slice(0, n);
    const ys = v.slice(n, n * 2);
    const nx = norm(xs);
    const ny = norm(ys);
    const pad = size * 0.12;
    const span = size - pad * 2;
    const out: Dot[] = [];
    for (let i = 0; i < n; i++) {
      const mag = Math.abs(xs[i]) + Math.abs(ys[i]);
      out.push({
        x: pad + nx[i] * span,
        y: pad + ny[i] * span,
        r: 0.6 + (i % 3) * 0.3,
        o: 0.3 + Math.min(0.6, mag * 6),
      });
    }
    return out;
  });
</script>

<svg
  viewBox="0 0 {size} {size}"
  width={size}
  height={size}
  class="rounded-lg border border-border bg-card"
  role="img"
  aria-label="Enrollment fingerprint"
>
  {#each points as d, i (i)}
    <circle cx={d.x} cy={d.y} r={d.r} class="fill-primary" opacity={d.o} />
  {/each}
</svg>
