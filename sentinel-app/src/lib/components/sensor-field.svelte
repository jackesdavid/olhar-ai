<script lang="ts">
  // The living background. A permanent, dim neutral-grey dot-lattice (the logo's
  // halftone) — the dots are always *there*, just not activated. Every few
  // seconds a pulse emanates from the focal centre and a WIDE, soft wavefront
  // spreads outward: each dot swells up and gently back down as the light passes,
  // so activation flows smoothly across neighbours (a sensor sweep), never a
  // sharp blink. Deliberately subtle. Fixed behind content; paused when hidden;
  // static field under prefers-reduced-motion.
  import { onMount } from "svelte";

  let canvas: HTMLCanvasElement;

  onMount(() => {
    // Bound to a non-nullable local rather than used through the narrowed original: every draw
    // below happens inside a nested function, and the narrowing from an early return does not
    // reliably survive into those — which is what had `check:web` red on six "possibly null".
    const maybeCtx = canvas.getContext("2d");
    if (!maybeCtx) return;
    const ctx: CanvasRenderingContext2D = maybeCtx;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const S = 44; // lattice spacing (px)
    const RGB = "140,140,140"; // neutral mid-grey dot lattice
    const REST = 0.05; // resting alpha — dots are always visible (dim)
    const PEAK = 0.3; // extra alpha at full activation
    const SPEED = 90; // px/s the wavefront spreads
    const BAND = 105; // px — WIDE & soft so activation rises/falls gently
    const LIFE = 5.2; // s — a pulse fades as it travels outward
    const SPAWN_MS = 5400; // gap between pulses (calm)

    let w = 0;
    let h = 0;
    let fx = 0; // focal origin
    let fy = 0;
    let dots: { x: number; y: number; cf: number }[] = [];
    let pulses: { x: number; y: number; t: number }[] = [];
    let lastSpawn = 0;
    let raf = 0;

    function rebuild() {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      w = window.innerWidth;
      h = window.innerHeight;
      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      fx = w / 2;
      fy = h * 0.4; // focal point a touch above centre
      const maxD = Math.hypot(Math.max(fx, w - fx), Math.max(fy, h - fy)) || 1;
      dots = [];
      const rowH = S * 0.866; // triangular lattice
      for (let r = 0, y = 0; y <= h + S; r++, y += rowH) {
        const off = r % 2 ? S / 2 : 0;
        for (let x = -off; x <= w + S; x += S) {
          const cf = 1 - 0.45 * (Math.hypot(x - fx, y - fy) / maxD);
          dots.push({ x, y, cf: Math.max(0.3, cf) });
        }
      }
    }

    function draw(now: number) {
      ctx.clearRect(0, 0, w, h);
      for (const d of dots) {
        let b = 0; // activation 0..1
        for (const p of pulses) {
          const age = (now - p.t) / 1000;
          const radius = age * SPEED;
          const e = (Math.hypot(d.x - p.x, d.y - p.y) - radius) / BAND;
          b += Math.exp(-e * e) * Math.max(0, 1 - age / LIFE); // soft swell, fades with travel
        }
        if (b > 1) b = 1;
        const a = (REST + b * PEAK) * d.cf;
        ctx.beginPath();
        ctx.arc(d.x, d.y, 0.8 + b * 1.2, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${RGB},${a.toFixed(3)})`;
        ctx.fill();
      }
    }

    function frame(now: number) {
      raf = requestAnimationFrame(frame);
      if (document.hidden) return; // smooth 60fps; per-frame work is light
      if (now - lastSpawn > SPAWN_MS) {
        // emanate from the focal centre with a little organic jitter
        pulses.push({ x: fx + (Math.random() - 0.5) * w * 0.18, y: fy + (Math.random() - 0.5) * h * 0.18, t: now });
        lastSpawn = now;
      }
      pulses = pulses.filter((p) => (now - p.t) / 1000 < LIFE);
      draw(now);
    }

    rebuild();
    const onResize = () => {
      rebuild();
      if (reduce) draw(0);
    };
    window.addEventListener("resize", onResize);

    if (reduce) draw(0);
    else raf = requestAnimationFrame(frame);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
    };
  });
</script>

<canvas bind:this={canvas} class="pointer-events-none fixed inset-0 -z-10" aria-hidden="true"></canvas>
