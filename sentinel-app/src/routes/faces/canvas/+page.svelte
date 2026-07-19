<script lang="ts">
  // Faces canvas (#119 Phase 2) — an infinite 2D map of similarity "islands". Server places the
  // islands (layout endpoint); this renders them on a <canvas> with pan/zoom, viewport culling, and
  // LOD (density blobs when zoomed out → face thumbnails when zoomed in). Click an island to label
  // the whole cluster via the same autocomplete as the grid. A full view, never a modal.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import ScanFace from "@lucide/svelte/icons/scan-face";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { data } from "$lib/data";
  import type { Island, Person } from "$lib/types";

  let canvasEl: HTMLCanvasElement;
  let wrapEl: HTMLDivElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let dpr = 1;

  let islands = $state<Island[]>([]);
  let people = $state<Person[]>([]);
  let loading = $state(true);
  let eps = $state(0.55);
  let epsTimer: ReturnType<typeof setTimeout> | undefined;
  let scope = $state<"unknown" | "known" | "all">("unknown");
  const SCOPES: Array<{ v: "unknown" | "known" | "all"; label: string }> = [
    { v: "unknown", label: "Unknown" },
    { v: "known", label: "Known" },
    { v: "all", label: "All" },
  ];

  // camera: world point at screen centre + scale (screen px per world unit)
  const cam = { x: 0, y: 0, scale: 1 };
  let selected = $state<Island | null>(null);
  let selScreen = $state({ x: 0, y: 0 }); // screen coords of the selected island (for the popup)
  let query = $state("");
  let busy = $state(false);

  const imgs = new Map<string, HTMLImageElement>(); // face id → crop (lazy)
  const FACE_R = 15; // face thumbnail radius, world units
  let dirty = true;

  onMount(() => {
    ctx = canvasEl.getContext("2d");
    dpr = window.devicePixelRatio || 1;
    const ro = new ResizeObserver(() => { resize(); dirty = true; });
    ro.observe(wrapEl);
    resize();
    load(true);
    let raf = 0;
    const loop = () => { if (dirty) { draw(); dirty = false; } raf = requestAnimationFrame(loop); };
    raf = requestAnimationFrame(loop);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); };
  });

  function resize() {
    if (!canvasEl || !wrapEl) return;
    const w = wrapEl.clientWidth, h = wrapEl.clientHeight;
    canvasEl.width = w * dpr; canvasEl.height = h * dpr;
    canvasEl.style.width = `${w}px`; canvasEl.style.height = `${h}px`;
  }
  function vw() { return canvasEl.width / dpr; }
  function vh() { return canvasEl.height / dpr; }
  function toScreen(wx: number, wy: number) {
    return { x: (wx - cam.x) * cam.scale + vw() / 2, y: (wy - cam.y) * cam.scale + vh() / 2 };
  }
  function toWorld(sx: number, sy: number) {
    return { x: (sx - vw() / 2) / cam.scale + cam.x, y: (sy - vh() / 2) / cam.scale + cam.y };
  }

  async function load(fit = false) {
    loading = true;
    try {
      const [lay, pp] = await Promise.all([data.faceLayout(eps, scope), data.listPeople()]);
      islands = lay.islands;
      people = pp;
      if (fit) fitView();
      dirty = true;
    } catch (e) {
      toast.error("Couldn't load canvas", { description: String(e) });
    } finally {
      loading = false;
    }
  }
  async function recluster(fit = false) {
    try {
      islands = (await data.faceLayout(eps, scope)).islands;
      selected = null;
      if (fit) fitView();
      dirty = true;
    } catch (e) {
      toast.error("Couldn't re-group", { description: String(e) });
    }
  }
  function setScope(v: "unknown" | "known" | "all") {
    if (v === scope) return;
    scope = v;
    recluster(true);
  }
  function onEps(v: number) { eps = v; clearTimeout(epsTimer); epsTimer = setTimeout(recluster, 300); }

  // Frame all islands into view.
  function fitView() {
    if (!islands.length) { cam.x = 0; cam.y = 0; cam.scale = 1; return; }
    let minx = Infinity, miny = Infinity, maxx = -Infinity, maxy = -Infinity;
    for (const is of islands) {
      minx = Math.min(minx, is.cx - is.radius); miny = Math.min(miny, is.cy - is.radius);
      maxx = Math.max(maxx, is.cx + is.radius); maxy = Math.max(maxy, is.cy + is.radius);
    }
    cam.x = (minx + maxx) / 2; cam.y = (miny + maxy) / 2;
    const pad = 80;
    const sx = (vw() - pad) / Math.max(maxx - minx, 1);
    const sy = (vh() - pad) / Math.max(maxy - miny, 1);
    cam.scale = Math.min(Math.max(Math.min(sx, sy), 0.05), 2);
  }

  function getImg(id: string): HTMLImageElement | null {
    let im = imgs.get(id);
    if (im) return im.complete && im.naturalWidth > 0 ? im : null;
    im = new Image();
    im.onload = () => { dirty = true; };
    im.src = data.faceImageUrl(id);
    imgs.set(id, im);
    return null;
  }

  function draw() {
    if (!ctx) return;
    const W = vw(), H = vh();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = "#0a0b0d";
    ctx.fillRect(0, 0, W, H);
    if (!islands.length) return;

    const detail = cam.scale >= 0.45; // LOD: thumbnails vs blobs
    const margin = 60;
    for (const is of islands) {
      const c = toScreen(is.cx, is.cy);
      const rS = is.radius * cam.scale;
      if (c.x + rS < -margin || c.x - rS > W + margin || c.y + rS < -margin || c.y - rS > H + margin) continue;

      if (!detail) {
        // density blob
        const g = ctx.createRadialGradient(c.x, c.y, 0, c.x, c.y, rS);
        g.addColorStop(0, "rgba(120,150,255,0.55)");
        g.addColorStop(1, "rgba(120,150,255,0.05)");
        ctx.fillStyle = g;
        ctx.beginPath(); ctx.arc(c.x, c.y, rS, 0, Math.PI * 2); ctx.fill();
        ctx.fillStyle = "rgba(255,255,255,0.85)";
        ctx.font = `${Math.max(11, Math.min(rS / 1.6, 26))}px ui-sans-serif, system-ui`;
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(String(is.size), c.x, c.y);
      } else {
        // faces as circular thumbnails
        for (const f of is.faces) {
          const p = toScreen(f.x, f.y);
          const fr = FACE_R * cam.scale;
          if (p.x + fr < 0 || p.x - fr > W || p.y + fr < 0 || p.y - fr > H) continue;
          const im = getImg(f.id);
          ctx.save();
          ctx.beginPath(); ctx.arc(p.x, p.y, fr, 0, Math.PI * 2); ctx.closePath(); ctx.clip();
          if (im) {
            ctx.drawImage(im, p.x - fr, p.y - fr, fr * 2, fr * 2);
          } else {
            ctx.fillStyle = "#1c1f26"; ctx.fillRect(p.x - fr, p.y - fr, fr * 2, fr * 2);
          }
          ctx.restore();
          ctx.lineWidth = 1;
          ctx.strokeStyle = "rgba(255,255,255,0.12)";
          ctx.beginPath(); ctx.arc(p.x, p.y, fr, 0, Math.PI * 2); ctx.stroke();
        }
      }

      // selection ring + label
      if (selected?.key === is.key) {
        ctx.strokeStyle = "#7aa2ff"; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(c.x, c.y, rS + 6, 0, Math.PI * 2); ctx.stroke();
      }
      // Labeled anchor → person name (brighter); unknown → suggestion or bare count.
      const label = is.label
        ? `${is.label} · ${is.size}`
        : is.suggestion
          ? `${is.suggestion.name} · ${is.size}`
          : `${is.size} face${is.size === 1 ? "" : "s"}`;
      ctx.fillStyle = is.label ? "rgba(160,190,255,0.95)" : "rgba(255,255,255,0.6)";
      ctx.font = `${is.label ? "600 " : ""}12px ui-sans-serif, system-ui`;
      ctx.textAlign = "center"; ctx.textBaseline = "top";
      ctx.fillText(label, c.x, c.y + rS + 6);
    }
  }

  // ---- interaction: pan / zoom / select ----
  let dragging = $state(false);
  let moved = false, lastX = 0, lastY = 0;
  function onPointerDown(e: PointerEvent) {
    canvasEl.setPointerCapture(e.pointerId);
    dragging = true; moved = false; lastX = e.clientX; lastY = e.clientY;
  }
  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    const dx = e.clientX - lastX, dy = e.clientY - lastY;
    if (Math.abs(dx) + Math.abs(dy) > 3) moved = true;
    cam.x -= dx / cam.scale; cam.y -= dy / cam.scale;
    lastX = e.clientX; lastY = e.clientY;
    if (selected) syncSelScreen();
    dirty = true;
  }
  function onPointerUp(e: PointerEvent) {
    dragging = false;
    if (!moved) hitTest(e);
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = canvasEl.getBoundingClientRect();
    const sx = e.clientX - rect.left, sy = e.clientY - rect.top;
    const before = toWorld(sx, sy);
    const factor = Math.exp(-e.deltaY * 0.0015);
    cam.scale = Math.min(Math.max(cam.scale * factor, 0.04), 6);
    const after = toWorld(sx, sy);
    cam.x += before.x - after.x; cam.y += before.y - after.y;
    if (selected) syncSelScreen();
    dirty = true;
  }
  function hitTest(e: PointerEvent) {
    const rect = canvasEl.getBoundingClientRect();
    const w = toWorld(e.clientX - rect.left, e.clientY - rect.top);
    let hit: Island | null = null;
    for (const is of islands) {
      const d = Math.hypot(w.x - is.cx, w.y - is.cy);
      if (d <= is.radius + 8) { hit = is; break; }
    }
    selected = hit;
    query = "";
    if (hit) syncSelScreen();
  }
  function syncSelScreen() {
    if (!selected) return;
    const s = toScreen(selected.cx, selected.cy - selected.radius);
    selScreen = { x: s.x, y: s.y };
  }

  // ---- labeling (reuses the grid's autocomplete + labelFaces) ----
  function matchingPeople(): Person[] {
    const q = query.trim().toLowerCase();
    return (q ? people.filter((p) => p.name.toLowerCase().includes(q)) : people).slice(0, 6);
  }
  function exactExists(): boolean {
    const q = query.trim().toLowerCase();
    return !!q && people.some((p) => p.name.toLowerCase() === q);
  }
  async function commit(target: { personId?: string; newPersonName?: string }, who: string) {
    if (!selected) return;
    busy = true;
    try {
      const res = await data.labelFaces(selected.faces.map((f) => f.id), target);
      toast.success(`Labeled ${res.labeled} face${res.labeled === 1 ? "" : "s"} as ${who}`);
      islands = islands.filter((x) => x.key !== selected!.key);
      selected = null; query = "";
      if (target.newPersonName) people = await data.listPeople();
      dirty = true;
    } catch (e) {
      toast.error("Couldn't label", { description: String(e) });
    } finally {
      busy = false;
    }
  }
  function pick(p: Person) { commit({ personId: p.id }, p.name); }
  function createNew() { const n = query.trim(); if (n) commit({ newPersonName: n }, n); }
  function confirmSuggestion() {
    if (selected?.suggestion) commit({ personId: selected.suggestion.person_id }, selected.suggestion.name);
  }
</script>

<div class="flex h-[calc(100vh-8rem)] flex-col gap-3">
  <div class="flex items-center justify-between gap-3">
    <div class="flex items-center gap-2">
      <ScanFace class="size-5" />
      <h1 class="text-lg font-semibold">Faces canvas</h1>
      <span class="text-sm text-muted-foreground">{islands.length} islands · similar faces sit together</span>
    </div>
    <div class="flex items-center gap-2">
      <div class="flex overflow-hidden rounded-md border border-border">
        {#each SCOPES as s (s.v)}
          <button
            class="px-3 py-1.5 text-xs transition-colors {scope === s.v ? 'bg-primary text-primary-foreground' : 'hover:bg-accent'}"
            onclick={() => setScope(s.v)}
          >{s.label}</button>
        {/each}
      </div>
      <div class="flex items-center gap-2 rounded-md border border-border px-3 py-1.5" title="Grouping distance (unknown faces only)">
        <span class="text-xs text-muted-foreground">Grouping</span>
        <input type="range" min="0.30" max="0.80" step="0.01" value={eps}
          oninput={(e) => onEps(parseFloat(e.currentTarget.value))} class="h-1 w-28 cursor-pointer accent-primary" />
        <span class="w-9 text-right text-xs tabular-nums">{eps.toFixed(2)}</span>
      </div>
      <Button variant="outline" onclick={() => load(true)} disabled={loading}>
        <RefreshCw class="size-4 {loading ? 'animate-spin' : ''}" /> Refresh
      </Button>
      <Button variant="outline" onclick={() => goto("/faces")}>
        <LayoutGrid class="size-4" /> Grid
      </Button>
    </div>
  </div>

  <div bind:this={wrapEl} class="relative flex-1 overflow-hidden rounded-xl border border-border bg-[#0a0b0d]">
    <canvas
      bind:this={canvasEl}
      class="block touch-none {dragging ? 'cursor-grabbing' : 'cursor-grab'}"
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onwheel={onWheel}
    ></canvas>

    {#if loading}
      <div class="pointer-events-none absolute inset-0 grid place-items-center text-sm text-white/60">Laying out faces…</div>
    {:else if islands.length === 0}
      <div class="absolute inset-0 grid place-items-center text-sm text-white/60">No unknown faces to place.</div>
    {/if}

    <!-- floating label popup anchored above the selected island -->
    {#if selected}
      <div
        class="absolute z-10 w-64 -translate-x-1/2 -translate-y-full rounded-lg border border-border bg-popover p-2 shadow-xl"
        style="left:{selScreen.x}px; top:{selScreen.y - 10}px"
      >
        <div class="mb-1 flex items-center justify-between px-1">
          <span class="text-xs font-medium">Label {selected.size} face{selected.size === 1 ? "" : "s"}</span>
          <button class="text-muted-foreground hover:text-foreground" onclick={() => (selected = null)}><X class="size-3.5" /></button>
        </div>
        {#if selected.suggestion}
          <button class="press mb-1 flex w-full items-center gap-2 rounded-md bg-primary px-2 py-1.5 text-xs text-primary-foreground disabled:opacity-50"
            disabled={busy} onclick={confirmSuggestion}>
            <Check class="size-3.5" /> Confirm {selected.suggestion.name} · {Math.round(selected.suggestion.confidence)}%
          </button>
        {/if}
        <!-- svelte-ignore a11y_autofocus -->
        <Input placeholder="Search or name…" bind:value={query} autofocus class="h-8 text-xs" />
        <div class="mt-1 overflow-hidden rounded-md border border-border">
          {#each matchingPeople() as p (p.id)}
            <button class="flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs hover:bg-accent" disabled={busy} onclick={() => pick(p)}>
              {p.name}
            </button>
          {/each}
          {#if query.trim() && !exactExists()}
            <button class="flex w-full items-center gap-2 border-t border-border px-2 py-1.5 text-left text-xs hover:bg-accent" disabled={busy} onclick={createNew}>
              <UserPlus class="size-3.5 text-primary" /> Create “{query.trim()}”
            </button>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>
