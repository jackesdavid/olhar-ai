<script lang="ts">
  // The living background: a faint emerald dot-lattice (the logo's halftone) with
  // periodic "pings" — wavefronts that ripple outward from random points and
  // brighten dots as they pass, like a sensor sweeping. Sits fixed behind all
  // content. Throttled to ~30fps, paused when the tab is hidden, static fallback
  // for prefers-reduced-motion. Canvas keeps it cheap regardless of dot count.
  import { onMount } from "svelte";

  let canvas: HTMLCanvasElement;

  onMount(() => {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const S = 44; // lattice spacing (px)
    const RGB = "52,211,153"; // emerald-400
    const SPEED = 155; // wavefront px/s
    const LIFE = 3.4; // ping lifetime (s)
    const BAND = 26; // wavefront thickness (px)
    const SPAWN_MS = 2300; // gap between pings

    let w = 0;
    let h = 0;
    let dots: { x: number; y: number; cf: number }[] = [];
    let pings: { x: number; y: number; t: number }[] = [];
    let lastSpawn = 0;
    let lastFrame = 0;
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

      dots = [];
      const rowH = S * 0.866; // triangular lattice
      const cx = w / 2;
      const cy = h * 0.4; // focal point a touch above center
      const maxD = Math.hypot(Math.max(cx, w - cx), Math.max(cy, h - cy));
      for (let r = 0, y = 0; y <= h + S; r++, y += rowH) {
        const off = r % 2 ? S / 2 : 0;
        for (let x = -off; x <= w + S; x += S) {
          const cf = 1 - 0.55 * (Math.hypot(x - cx, y - cy) / maxD); // center brighter
          dots.push({ x, y, cf: Math.max(0.15, cf) });
        }
      }
    }

    function drawStatic() {
      ctx.clearRect(0, 0, w, h);
      for (const d of dots) {
        ctx.beginPath();
        ctx.arc(d.x, d.y, 0.9, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${RGB},${(0.05 * d.cf).toFixed(3)})`;
        ctx.fill();
      }
    }

    function frame(now: number) {
      raf = requestAnimationFrame(frame);
      if (document.hidden) return;
      if (now - lastFrame < 33) return; // ~30fps
      lastFrame = now;

      if (now - lastSpawn > SPAWN_MS) {
        const bx = (Math.random() - 0.5) * 0.7 + 0.5;
        const by = (Math.random() - 0.5) * 0.7 + 0.45;
        pings.push({ x: bx * w, y: by * h, t: now });
        lastSpawn = now;
      }
      pings = pings.filter((p) => (now - p.t) / 1000 < LIFE);

      ctx.clearRect(0, 0, w, h);
      for (const d of dots) {
        let b = 0;
        for (const p of pings) {
          const age = (now - p.t) / 1000;
          const radius = age * SPEED;
          const edge = (Math.hypot(d.x - p.x, d.y - p.y) - radius) / BAND;
          b += Math.exp(-edge * edge) * Math.max(0, 1 - age / LIFE);
        }
        b = b > 1 ? 1 : b;
        const a = (0.05 + b * 0.6) * d.cf;
        if (a < 0.012) continue;
        ctx.beginPath();
        ctx.arc(d.x, d.y, 0.8 + b * 1.8, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${RGB},${a.toFixed(3)})`;
        ctx.fill();
      }
    }

    rebuild();
    const onResize = () => {
      rebuild();
      if (reduce) drawStatic();
    };
    window.addEventListener("resize", onResize);

    if (reduce) drawStatic();
    else raf = requestAnimationFrame(frame);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
    };
  });
</script>

<canvas bind:this={canvas} class="pointer-events-none fixed inset-0 -z-10" aria-hidden="true"></canvas>
