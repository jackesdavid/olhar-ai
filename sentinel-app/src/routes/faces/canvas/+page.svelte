<script lang="ts">
  // Faces canvas (#119 Phase 2) — an infinite 2D map of similarity "islands". Server places the
  // islands (layout endpoint); this renders them on a <canvas> with pan/zoom, viewport culling, and
  // LOD (density blobs when zoomed out → face thumbnails when zoomed in). Click an island to label
  // the whole cluster via the same autocomplete as the grid. A full view, never a modal.
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";
  import ScanFace from "@lucide/svelte/icons/scan-face";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Lasso from "@lucide/svelte/icons/lasso";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Frame from "@lucide/svelte/icons/frame";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { data } from "$lib/data";
  import type { Island, Person, FaceDetail } from "$lib/types";

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
  // Filters / caps (feed the layout query). null = uncapped.
  let maxPerIsland = $state<number | null>(null);
  let maxIslands = $state<number | null>(null);
  let minSize = $state<number | null>(null);
  let spread = $state(1); // inter-island spacing multiplier
  let since = $state(""); // yyyy-mm-dd
  let until = $state("");
  let filterTimer: ReturnType<typeof setTimeout> | undefined;
  // Auto-refresh (seconds; 0 = off), like the Events page.
  let autoSecs = $state(0);
  const AUTO_OPTS = [0, 5, 15, 30, 60] as const;
  // Lasso select → bulk label.
  let lassoMode = $state(false);
  let lassoPts = $state<{ x: number; y: number }[]>([]); // screen-space polygon
  let lassoIds = $state<string[]>([]); // faces enclosed by the last lasso
  let bulkQuery = $state("");
  let bulkBusy = $state(false);
  function activeFilters(): boolean {
    return maxPerIsland != null || maxIslands != null || minSize != null || spread !== 1 || !!since || !!until;
  }
  function layoutOpts() {
    return {
      eps,
      scope,
      maxPerIsland: maxPerIsland ?? undefined,
      maxIslands: maxIslands ?? undefined,
      minSize: minSize ?? undefined,
      spread: spread !== 1 ? spread : undefined,
      since: since ? `${since}T00:00:00` : undefined,
      until: until ? `${until}T23:59:59.999999999` : undefined,
    };
  }
  function onFilterChange() { clearTimeout(filterTimer); filterTimer = setTimeout(() => recluster(true), 300); }
  function numOrNull(v: string): number | null { return v ? Math.max(1, parseInt(v, 10)) : null; }
  function clearFilters() { maxPerIsland = null; maxIslands = null; minSize = null; spread = 1; since = ""; until = ""; recluster(true); }
  function onSpread(v: number) { spread = v; clearTimeout(epsTimer); epsTimer = setTimeout(recluster, 250); }
  function toggleLasso() { lassoMode = !lassoMode; lassoPts = []; if (!lassoMode) lassoIds = []; }
  let showBox = $state(false); // full frame + detection rectangle vs cropped face
  function toggleBox() { showBox = !showBox; imgs.clear(); attempts.clear(); pending.length = 0; dirty = true; }
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
  let confirmDel = $state(false); // two-step guard for the destructive delete
  // Face detail (hero) overlay — click a face → cinematic reveal of all its data + actions.
  let detail = $state<{ id: string; data: FaceDetail | null; from: { x: number; y: number; s: number }; box: boolean } | null>(null);
  let hero = $state<{ x: number; y: number; w: number; h: number } | null>(null); // animated hero-img rect
  let detailReady = $state(false); // gates the panel slide + backdrop fade
  let busyDetail = $state(false);
  // Facial landmark overlay — on-demand 5 keypoints drawn over the crop, toggled like Context.
  let pointsOn = $state(false);
  let pointsBusy = $state(false);
  let detailPoints = $state<{ pts: [number, number][]; w: number; h: number } | null>(null);
  // Which landmark pairs to connect into the biometric constellation (InsightFace kps order:
  // 0 right-eye, 1 left-eye, 2 nose, 3 right-mouth, 4 left-mouth).
  const MESH: [number, number][] = [[0, 1], [0, 2], [1, 2], [2, 3], [2, 4], [3, 4]];
  let faceQuery = $state(""); // "assign to" autocomplete inside the detail
  // In-flight fly animation of a re-matched face (plain var — read by the draw loop).
  let flying: { faceId: string; x0: number; y0: number; x1: number; y1: number; t0: number; img: HTMLImageElement | null } | null = null;
  const FLY_MS = 650;
  // Layout tween: on a re-cluster (spread/grouping change) islands slide from their old spot to the
  // new one instead of snapping. `animOff` = each island's current start-offset (decays to 0).
  const animOff = new Map<string, { x: number; y: number; t0: number }>();
  const LAYOUT_MS = 420;
  const ZERO = { x: 0, y: 0 };
  function islandOffset(key: string): { x: number; y: number } {
    const a = animOff.get(key);
    if (!a) return ZERO;
    const t = (performance.now() - a.t0) / LAYOUT_MS;
    if (t >= 1) { animOff.delete(key); return ZERO; }
    const e = 1 - Math.pow(1 - t, 3); // easeOutCubic → offset fades to 0
    return { x: a.x * (1 - e), y: a.y * (1 - e) };
  }

  const imgs = new Map<string, HTMLImageElement>(); // face id → crop (lazy)
  const attempts = new Map<string, number>(); // per-id load attempts (for retry/backoff)
  const pending: string[] = []; // ids waiting for a load slot
  let inflight = 0;
  const MAX_INFLIGHT = 8; // cap concurrent crop fetches so we don't burst MinIO into 404s
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
    const loop = () => { if (dirty || flying || animOff.size) { draw(); dirty = false; } raf = requestAnimationFrame(loop); };
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
      const [lay, pp] = await Promise.all([data.faceLayout(layoutOpts()), data.listPeople()]);
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
      const old = new Map(islands.map((is) => [is.key, { cx: is.cx, cy: is.cy }]));
      const next = (await data.faceLayout(layoutOpts())).islands;
      // Seed the slide: each surviving island starts at its old offset and eases to the new spot.
      const t0 = performance.now();
      animOff.clear();
      if (!fit) {
        for (const is of next) {
          const o = old.get(is.key);
          if (o && (o.cx !== is.cx || o.cy !== is.cy)) animOff.set(is.key, { x: o.cx - is.cx, y: o.cy - is.cy, t0 });
        }
      }
      islands = next;
      selected = null;
      if (fit) fitView();
      dirty = true;
    } catch (e) {
      toast.error("Couldn't re-group", { description: String(e) });
    }
  }
  // Auto-refresh: re-fetch the layout on the chosen interval (0 = off). Silent (no fit/toast churn).
  $effect(() => {
    if (autoSecs <= 0) return;
    const t = setInterval(() => { if (!detail && !lassoMode) recluster(false); }, autoSecs * 1000);
    return () => clearInterval(t);
  });
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
    const im = imgs.get(id);
    if (im) return im.complete && im.naturalWidth > 0 ? im : null;
    if (!attempts.has(id)) { attempts.set(id, 0); pending.push(id); pump(); }
    return null;
  }
  function pump() {
    while (inflight < MAX_INFLIGHT && pending.length) startLoad(pending.shift()!);
  }
  function startLoad(id: string) {
    inflight++;
    const im = new Image();
    im.onload = () => { inflight--; dirty = true; pump(); };
    im.onerror = () => {
      // A transient 404 under a load burst — the object is there; back off and retry.
      inflight--;
      const n = (attempts.get(id) ?? 0) + 1;
      attempts.set(id, n);
      if (n < 4) setTimeout(() => { pending.push(id); pump(); }, 250 * n);
      pump();
    };
    im.src = data.faceImageUrl(id, { box: showBox });
    imgs.set(id, im);
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
      const off = islandOffset(is.key); // layout-tween slide (spread/grouping change)
      const c = toScreen(is.cx + off.x, is.cy + off.y);
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
        // very subtle island disk — a visual reference for where each cluster sits
        ctx.fillStyle = "rgba(120,150,255,0.035)";
        ctx.beginPath(); ctx.arc(c.x, c.y, rS, 0, Math.PI * 2); ctx.fill();
        ctx.strokeStyle = "rgba(255,255,255,0.07)";
        ctx.lineWidth = 1;
        ctx.beginPath(); ctx.arc(c.x, c.y, rS, 0, Math.PI * 2); ctx.stroke();
        // faces as rounded-square thumbnails
        for (const f of is.faces) {
          if (flying && f.id === flying.faceId) continue; // it's mid-flight, drawn separately
          const p = toScreen(f.x + off.x, f.y + off.y);
          const fr = FACE_R * cam.scale;
          if (p.x + fr < 0 || p.x - fr > W || p.y + fr < 0 || p.y - fr > H) continue;
          const im = getImg(f.id);
          const rr = Math.max(2, fr * 0.3); // rounded-square corner
          ctx.save();
          ctx.beginPath(); ctx.roundRect(p.x - fr, p.y - fr, fr * 2, fr * 2, rr); ctx.clip();
          if (im) {
            ctx.drawImage(im, p.x - fr, p.y - fr, fr * 2, fr * 2);
          } else {
            ctx.fillStyle = "#1c1f26"; ctx.fillRect(p.x - fr, p.y - fr, fr * 2, fr * 2);
          }
          ctx.restore();
          ctx.lineWidth = 1;
          ctx.strokeStyle = "rgba(255,255,255,0.12)";
          ctx.beginPath(); ctx.roundRect(p.x - fr, p.y - fr, fr * 2, fr * 2, rr); ctx.stroke();
        }
      }

      // selection ring + label
      if (selected?.key === is.key) {
        ctx.strokeStyle = "#7aa2ff"; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.arc(c.x, c.y, rS + 6, 0, Math.PI * 2); ctx.stroke();
      }
      // Labeled anchor → person name (brighter). Unknown cluster: a suggestion is only a GUESS —
      // prefix "probably" + italic so it never reads as a confirmed identity; else a bare count.
      const label = is.label
        ? `${is.label} · ${is.size}`
        : is.suggestion
          ? `probably ${is.suggestion.name} · ${is.size}`
          : `${is.size} face${is.size === 1 ? "" : "s"}`;
      ctx.fillStyle = is.label ? "rgba(160,190,255,0.95)" : "rgba(255,255,255,0.6)";
      ctx.font = `${is.label ? "600 " : is.suggestion ? "italic " : ""}12px ui-sans-serif, system-ui`;
      ctx.textAlign = "center"; ctx.textBaseline = "top";
      ctx.fillText(label, c.x, c.y + rS + 6);
    }

    // a re-matched face flying to its new island (its landing triggers a recluster)
    if (flying) {
      const t = Math.min((performance.now() - flying.t0) / FLY_MS, 1);
      const e = 1 - Math.pow(1 - t, 3); // easeOutCubic
      const p = toScreen(flying.x0 + (flying.x1 - flying.x0) * e, flying.y0 + (flying.y1 - flying.y0) * e);
      const fr = FACE_R * cam.scale * (1 - 0.35 * e);
      const rr = Math.max(2, fr * 0.3);
      ctx.save();
      ctx.beginPath(); ctx.roundRect(p.x - fr, p.y - fr, fr * 2, fr * 2, rr); ctx.clip();
      if (flying.img && flying.img.complete && flying.img.naturalWidth > 0) {
        ctx.drawImage(flying.img, p.x - fr, p.y - fr, fr * 2, fr * 2);
      } else {
        ctx.fillStyle = "#2a3350"; ctx.fillRect(p.x - fr, p.y - fr, fr * 2, fr * 2);
      }
      ctx.restore();
      ctx.strokeStyle = "#7aa2ff"; ctx.lineWidth = 2;
      ctx.beginPath(); ctx.roundRect(p.x - fr, p.y - fr, fr * 2, fr * 2, rr); ctx.stroke();
      if (t >= 1) { flying = null; recluster(); }
    }
  }

  // ---- interaction: pan / zoom / select ----
  let dragging = $state(false);
  let moved = false, lastX = 0, lastY = 0;
  function relPt(e: PointerEvent) {
    const rect = canvasEl.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }
  function onPointerDown(e: PointerEvent) {
    canvasEl.setPointerCapture(e.pointerId);
    if (lassoMode) { lassoPts = [relPt(e)]; return; } // draw a selection lasso instead of panning
    dragging = true; moved = false; lastX = e.clientX; lastY = e.clientY;
  }
  function onPointerMove(e: PointerEvent) {
    if (lassoMode) { if (lassoPts.length) { lassoPts = [...lassoPts, relPt(e)]; dirty = true; } return; }
    if (!dragging) return;
    const dx = e.clientX - lastX, dy = e.clientY - lastY;
    if (Math.abs(dx) + Math.abs(dy) > 3) moved = true;
    cam.x -= dx / cam.scale; cam.y -= dy / cam.scale;
    lastX = e.clientX; lastY = e.clientY;
    if (selected) syncSelScreen();
    dirty = true;
  }
  function onPointerUp(e: PointerEvent) {
    if (lassoMode) { finishLasso(); return; }
    dragging = false;
    if (!moved) hitTest(e);
  }
  // Close the lasso and collect every face dot inside the polygon (screen-space, even-odd rule).
  function finishLasso() {
    const poly = lassoPts;
    lassoPts = [];
    if (poly.length < 3) { dirty = true; return; }
    const ids: string[] = [];
    for (const is of islands) for (const f of is.faces) {
      const p = toScreen(f.x, f.y);
      if (pointInPoly(p.x, p.y, poly)) ids.push(f.id);
    }
    lassoIds = ids;
    bulkQuery = "";
    if (!ids.length) toast.info("Lasso caught no faces — draw around some dots.");
    dirty = true;
  }
  function pointInPoly(x: number, y: number, poly: { x: number; y: number }[]): boolean {
    let inside = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const xi = poly[i].x, yi = poly[i].y, xj = poly[j].x, yj = poly[j].y;
      if (((yi > y) !== (yj > y)) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
    }
    return inside;
  }
  function matchingBulk(): Person[] {
    const q = bulkQuery.trim().toLowerCase();
    return people.filter((p) => !q || p.name.toLowerCase().includes(q)).slice(0, 6);
  }
  function bulkExact(): Person | undefined {
    const q = bulkQuery.trim().toLowerCase();
    return people.find((p) => p.name.toLowerCase() === q);
  }
  async function bulkLabel(target: { personId?: string; newPersonName?: string }, name: string) {
    if (!lassoIds.length) return;
    bulkBusy = true;
    try {
      await data.labelFaces(lassoIds, target);
      toast.success(`Labeled ${lassoIds.length} face${lassoIds.length === 1 ? "" : "s"} → ${name}`);
      lassoIds = []; lassoMode = false;
      await recluster(true);
    } catch (e) {
      toast.error("Couldn't label", { description: String(e) });
    } finally {
      bulkBusy = false;
    }
  }
  function cancelBulk() { lassoIds = []; }
  async function bulkDelete() {
    if (!lassoIds.length) return;
    bulkBusy = true;
    try {
      const { deleted } = await data.deleteFaces(lassoIds);
      toast.success(`Deleted ${deleted} face${deleted === 1 ? "" : "s"}`);
      lassoIds = []; lassoMode = false;
      await recluster(true);
    } catch (e) {
      toast.error("Couldn't delete", { description: String(e) });
    } finally {
      bulkBusy = false;
    }
  }
  async function bulkIgnore() {
    if (!lassoIds.length) return;
    bulkBusy = true;
    try {
      await data.ignoreFaces(lassoIds, true);
      toast.success(`Ignored ${lassoIds.length} face${lassoIds.length === 1 ? "" : "s"}`);
      lassoIds = []; lassoMode = false;
      await recluster(true);
    } catch (e) {
      toast.error("Couldn't ignore", { description: String(e) });
    } finally {
      bulkBusy = false;
    }
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
    // Zoomed in (faces visible): a click on a face opens its detail.
    if (cam.scale >= 0.45) {
      for (const is of islands) {
        if (Math.hypot(w.x - is.cx, w.y - is.cy) > is.radius + 8) continue;
        let best: { id: string; x: number; y: number } | null = null;
        let bd = FACE_R + 3;
        for (const f of is.faces) {
          const d = Math.hypot(w.x - f.x, w.y - f.y);
          if (d < bd) { bd = d; best = f; }
        }
        if (best) { openDetail(best); return; }
        break;
      }
    }
    // Otherwise (or clicking gaps / when zoomed out) → island-level label popup.
    let hit: Island | null = null;
    for (const is of islands) {
      if (Math.hypot(w.x - is.cx, w.y - is.cy) <= is.radius + 8) { hit = is; break; }
    }
    selected = hit;
    query = "";
    confirmDel = false;
    if (hit) syncSelScreen();
  }

  // ---- face detail (hero) ----
  function targetRect() { return { x: window.innerWidth - 480 + 16, y: 20, w: 448, h: 356 }; }
  async function openDetail(f: { id: string; x: number; y: number }) {
    selected = null;
    const cr = canvasEl.getBoundingClientRect();
    const p = toScreen(f.x, f.y);
    const fr = FACE_R * cam.scale;
    const from = { x: cr.left + p.x - fr, y: cr.top + p.y - fr, s: fr * 2 };
    detail = { id: f.id, data: null, from, box: showBox }; // inherit the canvas Context mode
    hero = { x: from.x, y: from.y, w: from.s, h: from.s };
    detailReady = false;
    faceQuery = "";
    pointsOn = false; detailPoints = null; // reset landmark overlay for the new face
    // two rAFs so the initial rect paints before we transition to the target
    requestAnimationFrame(() => requestAnimationFrame(() => { if (detail) { hero = targetRect(); detailReady = true; } }));
    try {
      const d = await data.faceDetail(f.id);
      if (detail?.id === f.id) detail = { ...detail, data: d };
    } catch (e) {
      toast.error("Couldn't load face", { description: String(e) });
    }
  }
  function closeDetail() {
    if (!detail) return;
    detailReady = false;
    hero = { x: detail.from.x, y: detail.from.y, w: detail.from.s, h: detail.from.s };
    setTimeout(() => { detail = null; hero = null; }, 340);
  }
  function toggleDetailBox() {
    if (!detail) return;
    pointsOn = false; // the points overlay is crop-space; showing the full frame must exit it
    detail = { ...detail, box: !detail.box };
  }
  function loadDims(url: string): Promise<{ w: number; h: number }> {
    return new Promise((resolve, reject) => {
      const im = new Image();
      im.onload = () => resolve({ w: im.naturalWidth, h: im.naturalHeight });
      im.onerror = () => reject(new Error("image load failed"));
      im.src = url;
    });
  }
  // Toggle the on-demand landmark overlay. Points are in crop coords, so force the crop view (not
  // the full frame). Fetches once per face; the SVG shares the crop's pixel viewBox so dots + image
  // scale together (no manual object-cover math).
  async function toggleDetailPoints() {
    if (!detail) return;
    pointsOn = !pointsOn;
    if (!pointsOn) return;
    if (detail.box) detail = { ...detail, box: false };
    if (detailPoints) return;
    pointsBusy = true;
    try {
      const [res, dims] = await Promise.all([
        data.faceLandmarks(detail.id),
        loadDims(data.faceImageUrl(detail.id, { box: false })),
      ]);
      detailPoints = { pts: res.landmarks, w: dims.w, h: dims.h };
    } catch (e) {
      detailPoints = { pts: [], w: 0, h: 0 };
      toast.error("Couldn't detect landmarks", { description: String(e) });
    } finally {
      pointsBusy = false;
    }
  }
  function matchingMove(): Person[] {
    const q = faceQuery.trim().toLowerCase();
    return (q ? people.filter((p) => p.name.toLowerCase().includes(q)) : people).slice(0, 6);
  }
  function moveExact(): boolean {
    const q = faceQuery.trim().toLowerCase();
    return !!q && people.some((p) => p.name.toLowerCase() === q);
  }
  function startFly(faceId: string, targetName: string) {
    let fd: { id: string; x: number; y: number } | null = null;
    for (const is of islands) { const f = is.faces.find((x) => x.id === faceId); if (f) { fd = f; break; } }
    const target = islands.find((is) => is.label === targetName) ?? null;
    if (fd) flying = { faceId, x0: fd.x, y0: fd.y, x1: target?.cx ?? fd.x, y1: target ? target.cy : fd.y - 320, t0: performance.now(), img: imgs.get(faceId) ?? null };
  }
  async function moveFaceTo(p: Person) {
    if (!detail) return;
    busyDetail = true;
    const faceId = detail.id;
    try {
      await data.reassignFace(faceId, p.id);
      toast.success(`Moved to ${p.name}`);
      startFly(faceId, p.name); // flies on the canvas; its landing triggers recluster
      closeDetail();
    } catch (e) {
      toast.error("Couldn't move", { description: String(e) });
    } finally {
      busyDetail = false;
    }
  }
  async function createFromDetail() {
    const n = faceQuery.trim();
    if (!n || !detail) return;
    busyDetail = true;
    const faceId = detail.id;
    try {
      await data.labelFaces([faceId], { newPersonName: n });
      toast.success(`Assigned to ${n}`);
      people = await data.listPeople();
      startFly(faceId, n);
      closeDetail();
    } catch (e) {
      toast.error("Couldn't assign", { description: String(e) });
    } finally {
      busyDetail = false;
    }
  }
  async function deleteFromDetail() {
    if (!detail) return;
    busyDetail = true;
    const faceId = detail.id;
    try {
      await data.deleteFaces([faceId]);
      toast.success("Face deleted");
      closeDetail();
      recluster();
    } catch (e) {
      toast.error("Couldn't delete", { description: String(e) });
    } finally {
      busyDetail = false;
    }
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
      selected = null; query = "";
      if (target.newPersonName) people = await data.listPeople();
      await recluster(true); // membership changed → recompute the similarity islands
    } catch (e) {
      toast.error("Couldn't label", { description: String(e) });
    } finally {
      busy = false;
    }
  }
  function pick(p: Person) { commit({ personId: p.id }, p.name); }
  function createNew() { const n = query.trim(); if (n) commit({ newPersonName: n }, n); }
  async function deleteIsland() {
    if (!selected) return;
    if (!confirmDel) { confirmDel = true; return; } // first click arms, second deletes
    busy = true;
    try {
      const res = await data.deleteFaces(selected.faces.map((f) => f.id));
      toast.success(`Deleted ${res.deleted} face${res.deleted === 1 ? "" : "s"}`);
      selected = null; confirmDel = false;
      await recluster(true); // the pile changed → recompute the similarity islands
    } catch (e) {
      toast.error("Couldn't delete", { description: String(e) });
    } finally {
      busy = false;
    }
  }
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
      <Button variant={showBox ? "default" : "outline"} onclick={toggleBox} title="Show the full frame with the detection box (context)">
        <Frame class="size-4" /> Context
      </Button>
      <Button variant={lassoMode ? "default" : "outline"} onclick={toggleLasso} title="Lasso-select faces to bulk-label / delete / ignore">
        <Lasso class="size-4" /> Lasso
      </Button>
      <select bind:value={autoSecs} class="h-9 rounded-md border border-border bg-background px-2 text-xs" title="Auto-refresh the layout on an interval">
        {#each AUTO_OPTS as s (s)}
          <option value={s}>{s === 0 ? "Manual" : `Auto ${s}s`}</option>
        {/each}
      </select>
      <Button variant="outline" onclick={() => load(true)} disabled={loading}>
        <RefreshCw class="size-4 {loading ? 'animate-spin' : ''}" /> Refresh
      </Button>
    </div>
  </div>

  <!-- Persistent filter/tuning bar — always visible (no popover) -->
  <div class="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs">
    <div class="flex items-center gap-2" title="Grouping distance (unknown faces only)">
      <SlidersHorizontal class="size-3.5 text-muted-foreground" />
      <span class="text-muted-foreground">Grouping</span>
      <input type="range" min="0.30" max="0.80" step="0.01" value={eps}
        oninput={(e) => onEps(parseFloat(e.currentTarget.value))} class="h-1 w-24 cursor-pointer accent-primary" />
      <span class="w-8 tabular-nums">{eps.toFixed(2)}</span>
    </div>
    <div class="flex items-center gap-2" title="Distance between islands">
      <span class="text-muted-foreground">Spread</span>
      <input type="range" min="0.5" max="3" step="0.1" value={spread}
        oninput={(e) => onSpread(parseFloat(e.currentTarget.value))} class="h-1 w-24 cursor-pointer accent-primary" />
      <span class="w-8 tabular-nums">{spread.toFixed(1)}×</span>
    </div>
    <label class="flex items-center gap-1.5"><span class="text-muted-foreground">Max/island</span>
      <input type="number" min="1" placeholder="all" value={maxPerIsland ?? ""} class="h-7 w-16 rounded border border-border bg-background px-2"
        oninput={(e) => { maxPerIsland = numOrNull(e.currentTarget.value); onFilterChange(); }} /></label>
    <label class="flex items-center gap-1.5"><span class="text-muted-foreground">Max islands</span>
      <input type="number" min="1" placeholder="all" value={maxIslands ?? ""} class="h-7 w-16 rounded border border-border bg-background px-2"
        oninput={(e) => { maxIslands = numOrNull(e.currentTarget.value); onFilterChange(); }} /></label>
    <label class="flex items-center gap-1.5"><span class="text-muted-foreground">Min size</span>
      <input type="number" min="1" placeholder="1" value={minSize ?? ""} class="h-7 w-16 rounded border border-border bg-background px-2"
        oninput={(e) => { minSize = numOrNull(e.currentTarget.value); onFilterChange(); }} /></label>
    <label class="flex items-center gap-1.5"><span class="text-muted-foreground">From</span>
      <input type="date" value={since} class="h-7 rounded border border-border bg-background px-2"
        oninput={(e) => { since = e.currentTarget.value; onFilterChange(); }} /></label>
    <label class="flex items-center gap-1.5"><span class="text-muted-foreground">To</span>
      <input type="date" value={until} class="h-7 rounded border border-border bg-background px-2"
        oninput={(e) => { until = e.currentTarget.value; onFilterChange(); }} /></label>
    {#if activeFilters()}
      <button class="text-muted-foreground underline-offset-2 hover:text-foreground hover:underline" onclick={clearFilters}>Clear</button>
    {/if}
  </div>

  <div bind:this={wrapEl} class="relative flex-1 overflow-hidden rounded-xl border border-border bg-[#0a0b0d]">
    <canvas
      bind:this={canvasEl}
      class="block touch-none {lassoMode ? 'cursor-crosshair' : dragging ? 'cursor-grabbing' : 'cursor-grab'}"
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onwheel={onWheel}
    ></canvas>

    {#if lassoMode && lassoPts.length > 1}
      <svg class="pointer-events-none absolute inset-0 z-10 h-full w-full">
        <polygon points={lassoPts.map((p) => `${p.x},${p.y}`).join(" ")} fill="rgba(125,215,255,.12)" stroke="rgba(125,215,255,.9)" stroke-width="1.5" stroke-dasharray="4 3" />
      </svg>
    {/if}

    {#if lassoIds.length}
      <div class="absolute left-1/2 top-4 z-20 w-80 -translate-x-1/2 rounded-lg border border-border bg-background p-3 shadow-2xl">
        <div class="flex items-center justify-between">
          <p class="text-sm font-medium">{lassoIds.length} face{lassoIds.length === 1 ? "" : "s"} selected</p>
          <button onclick={cancelBulk} class="text-muted-foreground hover:text-foreground" aria-label="cancel"><X class="size-4" /></button>
        </div>
        <p class="mt-1 text-xs text-muted-foreground">Label them all as…</p>
        <Input placeholder="Search or name…" bind:value={bulkQuery} class="mt-2 h-8 text-xs" />
        <div class="mt-1 max-h-44 overflow-y-auto rounded-md border border-border">
          {#each matchingBulk() as p (p.id)}
            <button class="flex w-full px-2 py-1.5 text-left text-xs hover:bg-accent" disabled={bulkBusy} onclick={() => bulkLabel({ personId: p.id }, p.name)}>{p.name}</button>
          {/each}
          {#if bulkQuery.trim() && !bulkExact()}
            <button class="flex w-full items-center gap-2 border-t border-border px-2 py-1.5 text-left text-xs hover:bg-accent" disabled={bulkBusy} onclick={() => bulkLabel({ newPersonName: bulkQuery.trim() }, bulkQuery.trim())}>
              <UserPlus class="size-3.5 text-primary" /> Create “{bulkQuery.trim()}”
            </button>
          {/if}
        </div>
        <div class="mt-2 flex gap-2">
          <button class="flex flex-1 items-center justify-center gap-1.5 rounded-md border border-border px-2 py-1.5 text-xs text-muted-foreground transition-colors hover:border-foreground hover:text-foreground" disabled={bulkBusy} onclick={bulkIgnore}>
            <EyeOff class="size-3.5" /> Ignore
          </button>
          <button class="flex flex-1 items-center justify-center gap-1.5 rounded-md border border-border px-2 py-1.5 text-xs text-muted-foreground transition-colors hover:border-destructive hover:text-destructive" disabled={bulkBusy} onclick={bulkDelete}>
            <Trash2 class="size-3.5" /> Delete
          </button>
        </div>
      </div>
    {/if}

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
        <!-- destructive: remove false-positive / far-away junk faces for good -->
        <button
          class="mt-1 flex w-full items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs transition-colors {confirmDel ? 'bg-destructive text-destructive-foreground' : 'text-muted-foreground hover:text-destructive'}"
          disabled={busy}
          onclick={deleteIsland}
        >
          <Trash2 class="size-3.5" /> {confirmDel ? `Delete ${selected.size} — confirm?` : "Delete faces"}
        </button>
      </div>
    {/if}
  </div>
</div>

<!-- face detail (hero) overlay — click a face → cinematic reveal of its data + actions -->
{#if detail}
  <div class="fixed inset-0 z-30">
    <button
      class="absolute inset-0 bg-black/75 transition-opacity duration-300"
      style:opacity={detailReady ? 1 : 0}
      onclick={closeDetail}
      aria-label="Close detail"
    ></button>

    {#if hero}
      {#if pointsOn && detailPoints && detailPoints.w > 0}
        {@const dim = Math.max(detailPoints.w, detailPoints.h)}
        {@const dense = detailPoints.pts.length > 12}
        {@const dotR = dim / (dense ? 240 : 110)}
        <!-- crop + landmark constellation share one pixel-space viewBox → they scale together -->
        <svg
          class="pointer-events-none fixed z-40 overflow-hidden rounded-xl shadow-2xl ring-1 ring-white/15"
          style="left:{hero.x}px; top:{hero.y}px; width:{hero.w}px; height:{hero.h}px"
          viewBox="0 0 {detailPoints.w} {detailPoints.h}"
          preserveAspectRatio="xMidYMid slice"
        >
          <image href={data.faceImageUrl(detail.id, { box: false })} width={detailPoints.w} height={detailPoints.h} />
          <!-- The 5-kps constellation lines only make sense for the 5-point order; a dense
               landmark set (106) is drawn as bare dots, movie-style. -->
          {#if !dense}
            {#each MESH as [a, b] (a + "-" + b)}
              {#if detailPoints.pts[a] && detailPoints.pts[b]}
                <line x1={detailPoints.pts[a][0]} y1={detailPoints.pts[a][1]} x2={detailPoints.pts[b][0]} y2={detailPoints.pts[b][1]} stroke="rgba(125,215,255,.3)" stroke-width={dim / 260} />
              {/if}
            {/each}
          {/if}
          {#each detailPoints.pts as p (p[0] + "," + p[1])}
            <circle cx={p[0]} cy={p[1]} r={dotR} fill="rgba(150,220,255,.55)" />
          {/each}
        </svg>
      {:else}
        <img
          src={data.faceImageUrl(detail.id, { box: detail.box })}
          alt="face"
          class="pointer-events-none fixed z-40 rounded-xl object-cover shadow-2xl ring-1 ring-white/15 transition-all duration-[360ms] ease-out"
          style="left:{hero.x}px; top:{hero.y}px; width:{hero.w}px; height:{hero.h}px"
        />
      {/if}
    {/if}

    <div
      class="fixed right-0 top-0 z-30 flex h-full w-[480px] flex-col overflow-y-auto border-l border-border bg-background p-4 shadow-2xl transition-transform duration-300 ease-out"
      style:transform={detailReady ? "translateX(0)" : "translateX(100%)"}
    >
      <div style="height:364px"></div>
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <p class="truncate text-lg font-semibold">
            {detail.data?.person_name ?? (detail.data && detail.data.person_id ? "Labeled" : "Unknown")}
          </p>
          <p class="text-xs text-muted-foreground">
            {detail.data ? new Date(detail.data.created_at).toLocaleString() : "loading…"}
          </p>
        </div>
        <button class="text-muted-foreground hover:text-foreground" onclick={closeDetail} aria-label="close"><X class="size-5" /></button>
      </div>

      <div class="mt-2 flex items-center gap-3 text-xs">
        <button class="flex items-center gap-1.5 text-muted-foreground hover:text-foreground" onclick={toggleDetailBox}>
          <Frame class="size-3.5" /> {detail.box ? "Show face crop" : "Show full frame + box"}
        </button>
        <button class="flex items-center gap-1.5 {pointsOn ? 'text-primary' : 'text-muted-foreground hover:text-foreground'}" disabled={pointsBusy} onclick={toggleDetailPoints}>
          <ScanFace class="size-3.5" /> {pointsBusy ? "Detecting…" : pointsOn ? "Hide points" : "Face points"}
        </button>
      </div>

      {#if detail.data}
        <dl class="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
          <div><dt class="text-muted-foreground">Camera</dt><dd class="truncate">{detail.data.camera || "—"}</dd></div>
          <div><dt class="text-muted-foreground">Source</dt><dd>{detail.data.source}</dd></div>
          <div><dt class="text-muted-foreground">Match confidence</dt><dd class="tabular-nums">{Math.round(detail.data.confidence)}%</dd></div>
          <div><dt class="text-muted-foreground">Face size</dt><dd class="tabular-nums">{(detail.data.face_ratio * 100).toFixed(1)}% of frame</dd></div>
          {#if detail.data.bbox}
            <div class="col-span-2"><dt class="text-muted-foreground">Detection box</dt><dd class="tabular-nums">[{detail.data.bbox.join(", ")}]</dd></div>
          {/if}
        </dl>
      {:else}
        <p class="mt-3 text-xs text-muted-foreground">Loading details…</p>
      {/if}

      <div class="mt-auto space-y-2 pt-4">
        <p class="text-xs font-medium text-muted-foreground">Assign this face to a person</p>
        <Input placeholder="Search or name…" bind:value={faceQuery} class="h-8 text-xs" />
        <div class="max-h-40 overflow-y-auto rounded-md border border-border">
          {#each matchingMove() as p (p.id)}
            <button class="flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs hover:bg-accent" disabled={busyDetail} onclick={() => moveFaceTo(p)}>
              {p.name}
            </button>
          {/each}
          {#if faceQuery.trim() && !moveExact()}
            <button class="flex w-full items-center gap-2 border-t border-border px-2 py-1.5 text-left text-xs hover:bg-accent" disabled={busyDetail} onclick={createFromDetail}>
              <UserPlus class="size-3.5 text-primary" /> Create “{faceQuery.trim()}”
            </button>
          {/if}
        </div>
        <button
          class="flex w-full items-center justify-center gap-1.5 rounded-md border border-border px-2 py-1.5 text-xs text-muted-foreground transition-colors hover:border-destructive hover:text-destructive"
          disabled={busyDetail}
          onclick={deleteFromDetail}
        >
          <Trash2 class="size-3.5" /> Delete face
        </button>
      </div>
    </div>
  </div>
{/if}
