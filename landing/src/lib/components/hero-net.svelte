<script lang="ts">
  // The hero's living centrepiece: an ink field of nodes wired by thin connections, with
  // signals travelling *inward* along the wires and converging on the focal eye — a vision
  // network gathering signal. Echoes the dot-field atom + the sensor "ping". Canvas 2D,
  // DPR-capped, paused when hidden or scrolled offscreen; a static graph under
  // prefers-reduced-motion. The landing is permanently Noir, so ink is #FAFAFA (250,250,250).
  import { onMount, tick } from "svelte";

  let canvas: HTMLCanvasElement;
  let host: HTMLElement;

  onMount(() => {
    const ctx = canvas.getContext("2d")!;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const INK = "250,250,250";

    type Node = { x: number; y: number; f: number; act: number; edges: number[] };
    type Edge = { a: number; b: number; len: number; lit: number };
    type Sig = { e: number; from: number; t: number; sp: number };

    let w = 0, h = 0, fx = 0, fy = 0, maxD = 1;
    let nodes: Node[] = [];
    let edges: Edge[] = [];
    let sigs: Sig[] = [];
    let raf = 0, lastSpawn = 0, prev = 0, running = true;

    const other = (e: Edge, n: number) => (e.a === n ? e.b : e.a);
    const focalDist = (n: number) => Math.hypot(nodes[n].x - fx, nodes[n].y - fy);

    function build() {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const rect = host.getBoundingClientRect();
      w = rect.width;
      h = rect.height;
      if (w < 2 || h < 2) return; // not laid out yet — the ResizeObserver will fire again
      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      fx = w / 2;
      fy = h * 0.34; // focal = the eye, upper-centre
      maxD = Math.hypot(Math.max(fx, w - fx), Math.max(fy, h - fy)) || 1;

      // Even coverage via a jittered grid (no clumping), count scaled to the area.
      const target = Math.max(46, Math.min(130, Math.round((w * h) / 11000)));
      const cols = Math.max(5, Math.round(Math.sqrt(target * (w / Math.max(1, h)))));
      const rows = Math.max(4, Math.round(target / cols));
      const cw = w / cols, ch = h / rows;
      nodes = [];
      for (let r = 0; r < rows; r++) {
        for (let c = 0; c < cols; c++) {
          const x = (c + 0.5) * cw + (Math.random() - 0.5) * cw * 0.72;
          const y = (r + 0.5) * ch + (Math.random() - 0.5) * ch * 0.72;
          const f = Math.max(0, 1 - Math.hypot(x - fx, y - fy) / maxD);
          nodes.push({ x, y, f, act: 0, edges: [] });
        }
      }
      nodes.push({ x: fx, y: fy, f: 1, act: 0, edges: [] }); // the focal eye node

      // Proximity graph: wire near neighbours (within R).
      const R = Math.hypot(cw, ch) * 1.42;
      edges = [];
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const len = Math.hypot(nodes[i].x - nodes[j].x, nodes[i].y - nodes[j].y);
          if (len < R) {
            const e = edges.length;
            edges.push({ a: i, b: j, len, lit: 0 });
            nodes[i].edges.push(e);
            nodes[j].edges.push(e);
          }
        }
      }
      sigs = [];
    }

    function spawnSignal() {
      const start = Math.floor(Math.random() * nodes.length);
      const es = nodes[start].edges;
      if (!es.length) return;
      sigs.push({ e: es[(Math.random() * es.length) | 0], from: start, t: 0, sp: 0.6 + Math.random() * 0.7 });
    }

    function step(dt: number) {
      for (let s = sigs.length - 1; s >= 0; s--) {
        const sig = sigs[s];
        const e = edges[sig.e];
        e.lit = 1;
        sig.t += ((sig.sp * 340) / Math.max(24, e.len)) * dt;
        if (sig.t < 1) continue;
        const arrived = other(e, sig.from);
        nodes[arrived].act = 1;
        // Route toward the eye: hop to the neighbour that most reduces distance to focal.
        const here = focalDist(arrived);
        let best = -1, bestD = here;
        for (const ne of nodes[arrived].edges) {
          const d = focalDist(other(edges[ne], arrived));
          if (d < bestD - 1) { bestD = d; best = ne; }
        }
        if (best >= 0 && here > 8) {
          sig.e = best;
          sig.from = arrived;
          sig.t = 0;
        } else {
          sigs.splice(s, 1); // absorbed at the eye (or a local minimum)
        }
      }
    }

    function draw() {
      ctx.clearRect(0, 0, w, h);
      // wires
      for (const e of edges) {
        const fa = (nodes[e.a].f + nodes[e.b].f) / 2;
        // resting wires stay dim so they never muddy the text; a passing signal lights its edge.
        const a = 0.014 + Math.pow(fa, 1.8) * 0.055 + e.lit * (0.26 + fa * 0.5);
        ctx.strokeStyle = `rgba(${INK},${a.toFixed(3)})`;
        ctx.lineWidth = 0.6 + e.lit * 0.9;
        ctx.beginPath();
        ctx.moveTo(nodes[e.a].x, nodes[e.a].y);
        ctx.lineTo(nodes[e.b].x, nodes[e.b].y);
        ctx.stroke();
        e.lit *= 0.9;
      }
      // signals — a bright travelling dot with a soft glow
      for (const sig of sigs) {
        const e = edges[sig.e];
        const from = nodes[sig.from], to = nodes[other(e, sig.from)];
        const x = from.x + (to.x - from.x) * sig.t;
        const y = from.y + (to.y - from.y) * sig.t;
        const g = ctx.createRadialGradient(x, y, 0, x, y, 10);
        g.addColorStop(0, `rgba(${INK},0.85)`);
        g.addColorStop(1, `rgba(${INK},0)`);
        ctx.fillStyle = g;
        ctx.beginPath();
        ctx.arc(x, y, 10, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = `rgba(${INK},1)`;
        ctx.beginPath();
        ctx.arc(x, y, 1.7, 0, Math.PI * 2);
        ctx.fill();
      }
      // nodes
      for (const n of nodes) {
        const a = Math.min(1, 0.06 + Math.pow(n.f, 1.6) * 0.5 + n.act * 0.5);
        const r = 0.7 + Math.pow(n.f, 1.4) * 1.7 + n.act * 1.9;
        ctx.fillStyle = `rgba(${INK},${a.toFixed(3)})`;
        ctx.beginPath();
        ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
        ctx.fill();
        n.act *= 0.93;
      }
    }

    function frame(now: number) {
      raf = requestAnimationFrame(frame);
      if (!running || document.hidden) { prev = now; return; }
      const dt = Math.min(0.05, (now - prev) / 1000) || 0.016;
      prev = now;
      if (now - lastSpawn > 130 && sigs.length < 28) { spawnSignal(); lastSpawn = now; }
      step(dt);
      draw();
    }

    // Initial build is synchronous (+ a post-layout retry via tick) so the field paints even in a
    // background tab — ResizeObserver and RAF are both tied to the rendering lifecycle and won't
    // fire while hidden. The ResizeObserver then handles genuine resizes on a visible tab.
    build();
    draw();
    tick().then(() => { build(); draw(); });
    if (!reduce) raf = requestAnimationFrame(frame);
    const ro = new ResizeObserver(() => { build(); draw(); });
    ro.observe(host);
    const io = new IntersectionObserver((es) => { running = es[0].isIntersecting; }, { threshold: 0 });
    io.observe(host);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      io.disconnect();
    };
  });
</script>

<div bind:this={host} class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
  <canvas bind:this={canvas} class="absolute inset-0"></canvas>
  <!-- edge vignette so the field dissolves into the page, keeping the text column clean -->
  <div class="absolute inset-0" style="background:radial-gradient(125% 75% at 50% 34%, transparent 34%, var(--background) 84%);"></div>
</div>
