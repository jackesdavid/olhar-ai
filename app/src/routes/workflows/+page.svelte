<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import { toast } from "svelte-sonner";

  import Plus from "@lucide/svelte/icons/plus";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import Square from "@lucide/svelte/icons/square";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Power from "@lucide/svelte/icons/power";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import Maximize from "@lucide/svelte/icons/maximize";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Terminal from "@lucide/svelte/icons/terminal";
  import X from "@lucide/svelte/icons/x";
  import Search from "@lucide/svelte/icons/search";
  import Link2 from "@lucide/svelte/icons/link-2";
  import Unplug from "@lucide/svelte/icons/unplug";
  import Upload from "@lucide/svelte/icons/upload";
  import ImageIcon from "@lucide/svelte/icons/image";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Select from "$lib/components/ui/select";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  // ---- types (the serialized core shapes) ----------------------------------
  type PortType = "text" | "num" | "bool" | "image" | "recipient";
  type CatPort = { name: string; type: PortType; required: boolean };
  type CatKind = {
    slug: string;
    label: string;
    category: string;
    inputs: CatPort[];
    outputs: CatPort[];
    default_config: Record<string, unknown>;
  };
  type Node = { id: number; kind: string; x: number; y: number; config: Record<string, unknown> };
  type Edge = { from: number; from_port: string; to: number; to_port: string };
  type Trigger = "Manual" | { Cron: { every_secs: number } } | { OnNewAlert: { min_level: string } };
  type Concurrency =
    | "SkipIfRunning"
    | { QueueSingle: { max_queued: number } }
    | { Concurrent: { max_concurrent: number } };
  type Graph = {
    id: number;
    name: string;
    enabled: boolean;
    trigger: Trigger;
    concurrency: Concurrency;
    run_orphans?: boolean;
    nodes: Node[];
    edges: Edge[];
    viewport?: { x: number; y: number; zoom: number };
  };
  type LogLine = { time: string; name: string; ok: boolean; summary: string };

  // ---- layout constants ----------------------------------------------------
  const NODE_W = 176;
  const HEAD_H = 26;
  const ROW_H = 22;
  const PAD = 8;

  // ---- state ---------------------------------------------------------------
  let graphs = $state<Graph[]>([]);
  let catalog = $state<Record<string, CatKind>>({});
  let view = $state<"list" | "editor">("list");
  let selectedGraphId = $state<number | null>(null);
  let selectedNodeId = $state<number | null>(null);
  let pending = $state<{ node: number; port: string } | null>(null);
  let loading = $state(true);
  let running = $state(false);
  let addOpen = $state(false);
  let consoleOpen = $state(false);
  let logs = $state<LogLine[]>([]);

  // canvas viewport (infinite pan/zoom)
  let pan = $state({ x: 40, y: 40 });
  let zoom = $state(1);
  // cursor in world coords — the live end of a wire being drawn
  let cursor = $state({ x: 0, y: 0 });

  // live references for node dropdowns + per-graph run activity (polled)
  let devices = $state<{ name: string; rtsp_url?: string | null }[]>([]);
  let recipients = $state<{ label: string; enabled: boolean }[]>([]);
  let activity = $state<Record<number, { running: number; current: number | null }>>({});

  // Per-node highlight controller (driven by workflow-node events): exactly ONE
  // node is "live" at a time, shown for at least MIN_HIGHLIGHT_MS, in sequence.
  // Fast nodes queue (so every node is still seen) and a finished node stops the
  // moment the next one starts — no overlap. Visual only.
  const MIN_HIGHLIGHT_MS = 700;
  let liveNode = $state<number | null>(null);
  let liveQueue: number[] = [];
  let liveTimer: ReturnType<typeof setTimeout> | null = null;
  let runEnded = false;

  function markLive(nodeId: number) {
    runEnded = false; // a node event means a run is active
    const tail = liveQueue.length ? liveQueue[liveQueue.length - 1] : liveNode;
    if (tail === nodeId) return; // already current / queued
    if (liveTimer === null) showLive(nodeId);
    else liveQueue.push(nodeId); // within the current node's minimum → queue it
  }
  function showLive(nodeId: number) {
    liveNode = nodeId;
    liveTimer = setTimeout(onMinElapsed, MIN_HIGHLIGHT_MS);
  }
  function onMinElapsed() {
    liveTimer = null;
    const next = liveQueue.shift();
    if (next !== undefined) showLive(next);
    else if (runEnded) liveNode = null;
    // else: keep the current node lit (still running) until the next event
  }
  function endLive() {
    // run finished/errored: let any queued min-holds drain, then stop.
    runEnded = true;
    if (liveTimer === null && liveQueue.length === 0) liveNode = null;
  }
  function clearLive() {
    if (liveTimer) clearTimeout(liveTimer);
    liveTimer = null;
    liveQueue = [];
    runEnded = false;
    liveNode = null;
  }

  // ---- delete confirmation (window.confirm is a no-op in the Tauri WebView) --
  let deleteOpen = $state(false);
  let deleteTarget = $state<Graph | null>(null);

  const graph = $derived(graphs.find((g) => g.id === selectedGraphId) ?? null);
  const selectedNode = $derived(graph?.nodes.find((n) => n.id === selectedNodeId) ?? null);
  const kinds = $derived(Object.values(catalog));

  // node palette: search + group by category
  let nodeSearch = $state("");
  const CATEGORY_ORDER = ["Cameras", "AI", "Alerts", "Recipients", "Text"];
  const nodeCategories = $derived.by(() => {
    const q = nodeSearch.trim().toLowerCase();
    const matched = kinds.filter(
      (k) =>
        !q ||
        k.label.toLowerCase().includes(q) ||
        k.category.toLowerCase().includes(q) ||
        k.slug.toLowerCase().includes(q),
    );
    const groups = new Map<string, CatKind[]>();
    for (const k of matched) {
      const list = groups.get(k.category) ?? [];
      list.push(k);
      groups.set(k.category, list);
    }
    const ordered: { name: string; kinds: CatKind[] }[] = [];
    for (const name of CATEGORY_ORDER) {
      const list = groups.get(name);
      if (list) {
        ordered.push({ name, kinds: list });
        groups.delete(name);
      }
    }
    for (const [name, list] of groups) ordered.push({ name, kinds: list });
    return ordered;
  });

  // run state for the open graph
  const act = $derived(selectedGraphId != null ? activity[selectedGraphId] : undefined);
  const isRunning = $derived((act?.running ?? 0) > 0);
  // A node (and the edge feeding it) is "live" only while it's the current node.
  function nodeLive(id: number): boolean {
    return liveNode === id;
  }

  // option lists for camera / recipient node dropdowns (the real items)
  const cameraOptions = $derived(devices.map((d) => ({ value: d.name, label: d.name })));
  const recipientOptions = $derived(recipients.map((r) => ({ value: r.label, label: r.label })));
  // Resolve a stored value to a real option — falls back to the FIRST item in the
  // list (what "default to first" means) when the stored value is empty/missing.
  function resolveOpt(opts: { value: string; label: string }[], stored: string) {
    return opts.some((o) => o.value === stored) ? stored : (opts[0]?.value ?? "");
  }

  // Property rows = the node's input ports (so wire-only ports like recipient /
  // image are always visible) + the legitimate static settings for that kind.
  // We use the catalog's default_config (the schema) — NOT the node's live
  // config — so stale keys from older schemas (e.g. notify_beacon's `target`)
  // are never rendered.
  const configRows = $derived.by(() => {
    if (!selectedNode) return [] as { key: string; port: CatPort | null }[];
    const nk = catalog[selectedNode.kind];
    const inputs = nk?.inputs ?? [];
    const rows: { key: string; port: CatPort | null }[] = inputs.map((p) => ({ key: p.name, port: p }));
    for (const key of Object.keys(nk?.default_config ?? {})) {
      if (!inputs.some((p) => p.name === key)) rows.push({ key, port: null });
    }
    return rows;
  });

  function runStatus(g: Graph): "running" | "idle" | "disabled" {
    if ((activity[g.id]?.running ?? 0) > 0) return "running";
    return g.enabled ? "idle" : "disabled";
  }

  function nodeHeight(n: Node): number {
    const k = catalog[n.kind];
    const rows = Math.max(k?.inputs.length ?? 0, k?.outputs.length ?? 0, 1);
    return HEAD_H + rows * ROW_H + PAD;
  }
  // Port Y *within* a node (local coords — the node <g> is translated to n.x/n.y).
  function portY(i: number) {
    return HEAD_H + i * ROW_H + ROW_H / 2;
  }
  // Absolute port Y (for drawing edges between nodes).
  function inPortY(n: Node, i: number) {
    return n.y + portY(i);
  }
  function outPortY(n: Node, i: number) {
    return n.y + portY(i);
  }
  function portIndex(node: Node, port: string, side: "in" | "out") {
    const k = catalog[node.kind];
    const list = side === "in" ? k?.inputs : k?.outputs;
    return list?.findIndex((p) => p.name === port) ?? -1;
  }
  function wiredInto(nodeId: number, port: string): Edge | undefined {
    return graph?.edges.find((e) => e.to === nodeId && e.to_port === port);
  }
  const portColor: Record<PortType, string> = {
    text: "#60a5fa",
    num: "#34d399",
    bool: "#fbbf24",
    image: "#f472b6",
    recipient: "#a78bfa",
  };
  // The colour of a port (used so a connection follows its data type).
  function portColorOf(node: Node, port: string, side: "in" | "out"): string {
    const list = side === "out" ? catalog[node.kind]?.outputs : catalog[node.kind]?.inputs;
    const p = list?.find((x) => x.name === port);
    return p ? portColor[p.type] : "#52796f";
  }

  // ---- navigation ----------------------------------------------------------
  // Persist the editor viewport (pan/zoom) to the store for the open graph, and
  // mirror it locally so re-opening restores without a reload.
  function saveViewport() {
    const id = selectedGraphId;
    if (id == null) return;
    const v = { x: pan.x, y: pan.y, zoom };
    const g = graphs.find((x) => x.id === id);
    if (g) g.viewport = v;
    invoke("set_graph_viewport", { id, x: v.x, y: v.y, zoom: v.zoom }).catch(() => {});
  }
  function openGraph(id: number) {
    saveViewport();
    selectedGraphId = id;
    selectedNodeId = null;
    pending = null;
    const v = graphs.find((x) => x.id === id)?.viewport;
    pan = v ? { x: v.x, y: v.y } : { x: 40, y: 40 };
    zoom = v?.zoom ?? 1;
    clearLive();
    view = "editor";
  }
  function backToList() {
    saveViewport();
    view = "list";
    selectedNodeId = null;
    pending = null;
    clearLive();
  }

  // ---- load ----------------------------------------------------------------
  async function reload() {
    graphs = await invoke<Graph[]>("load_graphs");
    if (selectedGraphId !== null && !graphs.some((g) => g.id === selectedGraphId)) {
      selectedGraphId = null;
    }
  }

  async function loadRefs() {
    try {
      const [d, r] = await Promise.all([
        invoke<{ name: string; rtsp_url?: string | null }[]>("list_devices"),
        invoke<{ label: string; enabled: boolean }[]>("list_beacons"),
      ]);
      devices = d;
      recipients = r;
    } catch (err) {
      console.error("Failed to load devices/recipients", err);
    }
  }

  onMount(() => {
    (async () => {
      try {
        const cat = await invoke<{ kinds: CatKind[] }>("node_catalog");
        catalog = Object.fromEntries(cat.kinds.map((k) => [k.slug, k]));
        await reload();
        await loadRefs();
      } catch (err) {
        toast.error("Failed to load workflows", { description: String(err) });
      } finally {
        loading = false;
      }
    })();
    const un = listen<{ graph: number; name: string; ok: boolean; summary: string }>(
      "workflow-run",
      (e) => {
        running = false;
        const { graph: gid, name, ok, summary } = e.payload;
        // Run finished (ok or error) → drain remaining highlights, then stop.
        if (gid === selectedGraphId) endLive();
        logs = [{ time: new Date().toLocaleTimeString(), name, ok, summary }, ...logs].slice(0, 200);
        if (ok) toast.success(`Workflow "${name}"`, { description: summary });
        else toast.error(`Workflow "${name}" failed`, { description: summary });
      },
    );
    // Per-node execution events (real-time, catch every node).
    const unNode = listen<{ graph: number; node: number }>("workflow-node", (e) => {
      if (e.payload.graph === selectedGraphId) markLive(e.payload.node);
    });
    // Poll run activity (running graphs + status) for the lock + list badges.
    const poll = setInterval(async () => {
      try {
        activity = await invoke("run_states");
      } catch {
        /* ignore transient errors */
      }
    }, 700);
    return () => {
      saveViewport(); // persist the viewport when leaving the page too
      un.then((u) => u());
      unNode.then((u) => u());
      clearInterval(poll);
    };
  });

  // ---- graph CRUD ----------------------------------------------------------
  async function createWorkflow() {
    try {
      const id = await invoke<number>("create_graph", { name: "Untitled" });
      await reload();
      openGraph(id); // opens the editor — rename it inline in Configuration
    } catch (err) {
      toast.error("Couldn't create workflow", { description: String(err) });
    }
  }
  async function renameInline(name: string) {
    if (!graph) return;
    const n = name.trim();
    if (!n || n === graph.name) return;
    try {
      await invoke("rename_graph", { id: graph.id, name: n });
      await reload();
    } catch (err) {
      toast.error("Rename failed", { description: String(err) });
    }
  }
  function openDelete(g: Graph) {
    deleteTarget = g;
    deleteOpen = true;
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    const wasOpen = selectedGraphId === deleteTarget.id;
    try {
      await invoke("delete_graph", { id: deleteTarget.id });
      deleteOpen = false;
      deleteTarget = null;
      await reload();
      if (wasOpen) backToList();
    } catch (err) {
      toast.error("Failed to delete", { description: String(err) });
    }
  }
  async function toggleEnabled(g: Graph, enabled: boolean) {
    g.enabled = enabled; // optimistic — reflect the flip instantly
    try {
      await invoke("set_graph_enabled", { id: g.id, enabled });
      await reload();
    } catch (err) {
      g.enabled = !enabled; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function toggleRunOrphans(g: Graph, run_orphans: boolean) {
    g.run_orphans = run_orphans; // optimistic
    try {
      await invoke("set_graph_run_orphans", { id: g.id, runOrphans: run_orphans });
      await reload();
    } catch (err) {
      g.run_orphans = !run_orphans; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function runNow(g: Graph) {
    running = true;
    if (!consoleOpen) consoleOpen = true;
    try {
      const msg = await invoke<string>("run_graph", { id: g.id });
      toast.info(msg);
    } catch (err) {
      running = false;
      toast.error("Run", { description: String(err) });
    }
  }

  // ---- trigger / concurrency ----------------------------------------------
  function triggerKind(t: Trigger): "manual" | "cron" | "on_new_alert" {
    if (t === "Manual") return "manual";
    return "Cron" in t ? "cron" : "on_new_alert";
  }
  function triggerSecs(t: Trigger): number {
    return typeof t === "object" && "Cron" in t ? t.Cron.every_secs : 60;
  }
  function triggerLevel(t: Trigger): string {
    return typeof t === "object" && "OnNewAlert" in t ? t.OnNewAlert.min_level : "info";
  }
  function triggerSummary(t: Trigger): string {
    if (t === "Manual") return "Manual";
    if ("Cron" in t) return `Every ${t.Cron.every_secs}s`;
    return `On alert ≥ ${t.OnNewAlert.min_level}`;
  }
  function concKind(c: Concurrency): "skip" | "queue" | "concurrent" {
    if (c === "SkipIfRunning") return "skip";
    return "QueueSingle" in c ? "queue" : "concurrent";
  }
  function concMax(c: Concurrency): number {
    if (typeof c !== "object") return 1;
    return "QueueSingle" in c ? c.QueueSingle.max_queued : c.Concurrent.max_concurrent;
  }
  async function applyTrigger(g: Graph, kind: string, everySecs: number, minLevel: string) {
    await invoke("set_graph_trigger", { id: g.id, kind, everySecs, minLevel });
    await reload();
  }
  async function applyConcurrency(g: Graph, kind: string, max: number) {
    await invoke("set_graph_concurrency", { id: g.id, kind, max });
    await reload();
  }

  // ---- node + edge ops -----------------------------------------------------
  async function addNodeKind(slug: string) {
    if (!graph || isRunning) return;
    const n = graph.nodes.length;
    const x = 80 + (n % 5) * 48;
    const y = 70 + (n % 5) * 40;
    const id = await invoke<number>("add_node", { graphId: graph.id, kind: slug, x, y });
    // Default camera/recipient configs to the actual first item in each list.
    const cfg = { ...(catalog[slug]?.default_config ?? {}) };
    let patched = false;
    if ("camera" in cfg && devices[0]) {
      cfg.camera = devices[0].name;
      patched = true;
    }
    if ("recipient" in cfg && recipients[0]) {
      cfg.recipient = recipients[0].label;
      patched = true;
    }
    if (patched) await invoke("set_node_config", { graphId: graph.id, nodeId: id, config: cfg });
    selectedNodeId = id;
    addOpen = false;
    await reload();
  }

  function comingSoon(what: string) {
    toast.info(`${what} — coming soon`);
  }
  async function removeNode(n: Node) {
    if (!graph) return;
    await invoke("remove_node", { graphId: graph.id, nodeId: n.id });
    if (selectedNodeId === n.id) selectedNodeId = null;
    await reload();
  }
  async function removeEdge(idx: number) {
    if (!graph) return;
    await invoke("remove_edge", { graphId: graph.id, idx });
    await reload();
  }
  // Drop the wire feeding a node's input port so the user can fill it manually.
  async function unlinkPort(nodeId: number, port: string) {
    if (!graph || isRunning) return;
    const idx = graph.edges.findIndex((e) => e.to === nodeId && e.to_port === port);
    if (idx >= 0) await removeEdge(idx);
  }
  function clickOutPort(node: Node, port: string) {
    if (isRunning) return;
    pending = { node: node.id, port };
    // seed the wire's free end at the port so it starts with zero length
    cursor = { x: node.x + NODE_W, y: outPortY(node, Math.max(0, portIndex(node, port, "out"))) };
  }
  async function clickInPort(node: Node, port: string) {
    if (!graph || isRunning) return;
    const existing = graph.edges.findIndex((e) => e.to === node.id && e.to_port === port);
    if (existing >= 0) {
      await removeEdge(existing);
      return;
    }
    if (!pending) {
      toast.info("Pick an output port first, then this input.");
      return;
    }
    try {
      await invoke("add_edge", {
        graphId: graph.id,
        from: pending.node,
        fromPort: pending.port,
        to: node.id,
        toPort: port,
      });
      pending = null;
      await reload();
    } catch (err) {
      toast.error("Can't wire that", { description: String(err) });
    }
  }
  async function saveNodeConfig(n: Node, config: Record<string, unknown>) {
    if (!graph) return;
    await invoke("set_node_config", { graphId: graph.id, nodeId: n.id, config });
    await reload();
  }
  // Pick an image file from disk for an image port (stored as a path literal).
  async function pickImage(n: Node, key: string) {
    if (isRunning) return;
    const picked = await openFileDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
    });
    if (typeof picked === "string") {
      await saveNodeConfig(n, { ...n.config, [key]: picked });
    }
  }
  const fileName = (p: string) => p.split(/[\\/]/).pop() || p;

  // ---- canvas: infinite pan / zoom + node drag -----------------------------
  let svgEl: SVGSVGElement | undefined;
  let drag: { id: number; offX: number; offY: number } | null = null;
  let panning: { sx: number; sy: number; ox: number; oy: number } | null = null;

  // Screen → world (the <g> transform is translate(pan) scale(zoom)).
  function worldPoint(e: PointerEvent | WheelEvent) {
    const r = svgEl!.getBoundingClientRect();
    return { x: (e.clientX - r.left - pan.x) / zoom, y: (e.clientY - r.top - pan.y) / zoom };
  }
  function startDrag(e: PointerEvent, n: Node) {
    selectedNodeId = n.id; // selecting to view props is fine
    if (isRunning) return; // …but no moving while running
    const wp = worldPoint(e);
    drag = { id: n.id, offX: wp.x - n.x, offY: wp.y - n.y };
  }
  function onCanvasDown(e: PointerEvent) {
    if (e.target !== e.currentTarget) return; // only grab empty canvas, not a node
    if (pending) {
      pending = null; // clicking empty space cancels the wire being drawn
      return;
    }
    panning = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
    svgEl?.setPointerCapture?.(e.pointerId);
  }
  function onCanvasMove(e: PointerEvent) {
    if (pending) cursor = worldPoint(e); // drag the open wire end with the cursor
    if (panning) {
      pan = { x: panning.ox + (e.clientX - panning.sx), y: panning.oy + (e.clientY - panning.sy) };
      return;
    }
    if (drag && graph) {
      const wp = worldPoint(e);
      const n = graph.nodes.find((x) => x.id === drag!.id);
      if (n) {
        n.x = Math.round(wp.x - drag.offX);
        n.y = Math.round(wp.y - drag.offY);
      }
    }
  }
  async function onCanvasUp() {
    if (panning) {
      panning = null;
      return;
    }
    if (drag && graph) {
      const id = drag.id;
      const n = graph.nodes.find((x) => x.id === id);
      drag = null;
      if (n) await invoke("move_node", { graphId: graph.id, nodeId: id, x: n.x, y: n.y });
    }
  }
  function clampZoom(z: number) {
    return Math.min(2.5, Math.max(0.35, z));
  }
  function zoomBy(f: number) {
    zoom = clampZoom(Math.round(zoom * f * 100) / 100);
  }
  function onWheel(e: WheelEvent) {
    const r = svgEl!.getBoundingClientRect();
    const cx = e.clientX - r.left;
    const cy = e.clientY - r.top;
    const nz = clampZoom(zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
    // keep the world point under the cursor fixed while zooming
    pan = { x: cx - ((cx - pan.x) / zoom) * nz, y: cy - ((cy - pan.y) / zoom) * nz };
    zoom = nz;
  }
  // Attach the wheel handler as a PASSIVE listener (no preventDefault) — better
  // scroll performance; the canvas has nothing to scroll so passive is safe.
  function passiveWheel(node: SVGSVGElement) {
    const h = (e: WheelEvent) => onWheel(e);
    node.addEventListener("wheel", h, { passive: true });
    return { destroy: () => node.removeEventListener("wheel", h) };
  }
  function fitView() {
    if (!graph || graph.nodes.length === 0 || !svgEl) {
      pan = { x: 40, y: 40 };
      zoom = 1;
      return;
    }
    const minX = Math.min(...graph.nodes.map((n) => n.x)) - 60;
    const minY = Math.min(...graph.nodes.map((n) => n.y)) - 60;
    const maxX = Math.max(...graph.nodes.map((n) => n.x + NODE_W)) + 60;
    const maxY = Math.max(...graph.nodes.map((n) => n.y + nodeHeight(n))) + 60;
    const r = svgEl.getBoundingClientRect();
    const z = clampZoom(Math.min(r.width / (maxX - minX), r.height / (maxY - minY), 1.5));
    zoom = z;
    pan = {
      x: (r.width - (maxX - minX) * z) / 2 - minX * z,
      y: (r.height - (maxY - minY) * z) / 2 - minY * z,
    };
  }

  const LEVELS = ["debug", "info", "warn", "error"];
  const TRIGGER_OPTS = [
    { value: "manual", label: "Manual (Run-now only)" },
    { value: "cron", label: "Cron (every N seconds)" },
    { value: "on_new_alert", label: "On new alert" },
  ];
  const CONC_OPTS = [
    { value: "skip", label: "Skip if running" },
    { value: "queue", label: "Queue (max N)" },
    { value: "concurrent", label: "Concurrent (max N)" },
  ];
  function labelOf(opts: { value: string; label: string }[], v: string) {
    return opts.find((o) => o.value === v)?.label ?? v;
  }
</script>

<div class="h-full">
  {#if loading}
    <div class="flex h-full items-center justify-center">
      <Loader label="Loading workflows…" />
    </div>

    <!-- ════════════════════ LIST ════════════════════ -->
  {:else if view === "list"}
    <div class="h-full overflow-y-auto">
      <div class="mx-auto max-w-6xl space-y-6 p-6">
      <div class="flex items-center justify-between">
        <div>
          <h1 class="text-2xl font-semibold tracking-tight">Workflows</h1>
          <p class="text-sm text-muted-foreground">
            Graph automations that run on a trigger or on demand.
          </p>
        </div>
        <Button onclick={createWorkflow}><Plus class="size-4" /> New workflow</Button>
      </div>

      <Card.Root>
        <Card.Content class="pt-6">
          {#if graphs.length === 0}
            <EmptyState
              title="No workflows yet"
              description="Create one to chain cameras, alerts, vision, and recipients into an automation."
            >
              <Button onclick={createWorkflow}><Plus class="size-4" /> New workflow</Button>
            </EmptyState>
          {:else}
            <Table.Root>
              <Table.Header>
                <Table.Row>
                  <Table.Head>Name</Table.Head>
                  <Table.Head>Trigger</Table.Head>
                  <Table.Head class="w-20 text-center">Nodes</Table.Head>
                  <Table.Head class="w-28">Status</Table.Head>
                  <Table.Head class="w-24">Enabled</Table.Head>
                  <Table.Head class="w-28 text-right">Actions</Table.Head>
                </Table.Row>
              </Table.Header>
              <Table.Body>
                {#each graphs as g (g.id)}
                  <Table.Row
                    class="cursor-pointer transition-colors hover:bg-primary/5"
                    onclick={() => openGraph(g.id)}
                  >
                    {@const st = runStatus(g)}
                    <Table.Cell class="font-medium">
                      <span class="flex items-center gap-2">
                        <StatusDot state={g.enabled ? "online" : "offline"} />
                        {g.name}
                      </span>
                    </Table.Cell>
                    <Table.Cell class="text-muted-foreground">{triggerSummary(g.trigger)}</Table.Cell>
                    <Table.Cell class="text-center tabular-nums text-muted-foreground">
                      {g.nodes.length}
                    </Table.Cell>
                    <Table.Cell>
                      {#if st === "running"}
                        <span
                          class="inline-flex items-center gap-1.5 rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-[11px] font-medium text-primary"
                        >
                          <span class="size-1.5 rounded-full bg-primary status-live"></span> Running
                        </span>
                      {:else if st === "idle"}
                        <span class="text-xs text-muted-foreground">Idle</span>
                      {:else}
                        <span class="text-xs text-muted-foreground/60">Disabled</span>
                      {/if}
                    </Table.Cell>
                    <Table.Cell onclick={(e) => e.stopPropagation()}>
                      <Switch checked={g.enabled} onCheckedChange={(v) => toggleEnabled(g, v)} />
                    </Table.Cell>
                    <Table.Cell class="text-right" onclick={(e) => e.stopPropagation()}>
                      <div class="flex items-center justify-end gap-1">
                        <Button variant="ghost" size="icon" class="size-8 text-primary" title="Run now" onclick={() => runNow(g)}>
                          <Play class="size-4" />
                        </Button>
                        <Button variant="ghost" size="icon" class="size-8" title="Delete" onclick={() => openDelete(g)}>
                          <Trash2 class="size-4" />
                        </Button>
                      </div>
                    </Table.Cell>
                  </Table.Row>
                {/each}
              </Table.Body>
            </Table.Root>
          {/if}
        </Card.Content>
      </Card.Root>
      </div>
    </div>

    <!-- ════════════════════ EDITOR ════════════════════ -->
  {:else if graph}
    <div class="flex h-full flex-col gap-3 p-4">
      <!-- header -->
      <div class="flex shrink-0 items-center gap-3">
        <Button variant="outline" size="sm" onclick={backToList}>
          <ArrowLeft class="size-4" /> Workflows
        </Button>
        <ChevronRight class="size-4 text-muted-foreground" />
        <span class="flex items-center gap-2">
          <StatusDot state={graph.enabled ? "online" : "offline"} />
          <span class="text-sm font-medium">{graph.name}</span>
          <span class="text-xs text-muted-foreground">· {graph.nodes.length} nodes</span>
        </span>
        {#if isRunning}
          <span
            class="flex items-center gap-1.5 rounded-full border border-primary/30 bg-primary/10 px-2.5 py-0.5 text-[11px] font-medium text-primary"
          >
            <span class="size-1.5 rounded-full bg-primary status-live"></span>
            Running — editing locked
          </span>
        {/if}
      </div>

      <!-- row 1: editor (80%) + side panel (20%) -->
      <div class="flex min-h-0 flex-1 gap-3">
        <!-- col 1: the node editor -->
        <div class="relative min-w-0 basis-4/5">
          <div
            class="absolute inset-0 overflow-hidden rounded-xl border border-border bg-card/20 bg-[radial-gradient(circle,_rgba(52,211,153,0.12)_1px,_transparent_1px)]"
            style="background-size: {18 * zoom}px {18 * zoom}px; background-position: {pan.x}px {pan.y}px;"
          >
            <svg
              bind:this={svgEl}
              use:passiveWheel
              class="block size-full {panning ? 'cursor-grabbing' : 'cursor-grab'}"
              onpointerdown={onCanvasDown}
              onpointermove={onCanvasMove}
              onpointerup={onCanvasUp}
              onpointerleave={onCanvasUp}
              role="presentation"
            >
              <defs>
                <filter id="nodeGlow" x="-40%" y="-40%" width="180%" height="180%">
                  <feDropShadow dx="0" dy="0" stdDeviation="5" flood-color="#10b981" flood-opacity="0.5" />
                </filter>
              </defs>

              <g transform="translate({pan.x} {pan.y}) scale({zoom})">
              <!-- edges -->
              {#each graph.edges as e, idx (idx)}
                {@const fromN = graph.nodes.find((n) => n.id === e.from)}
                {@const toN = graph.nodes.find((n) => n.id === e.to)}
                {#if fromN && toN}
                  {@const x1 = fromN.x + NODE_W}
                  {@const y1 = outPortY(fromN, Math.max(0, portIndex(fromN, e.from_port, "out")))}
                  {@const x2 = toN.x}
                  {@const y2 = inPortY(toN, Math.max(0, portIndex(toN, e.to_port, "in")))}
                  {@const eLive = nodeLive(e.to)}
                  {@const eColor = portColorOf(fromN, e.from_port, "out")}
                  <path
                    d={`M ${x1} ${y1} C ${x1 + 60} ${y1}, ${x2 - 60} ${y2}, ${x2} ${y2}`}
                    fill="none"
                    stroke={eColor}
                    stroke-width={eLive ? 3 : 2}
                    stroke-opacity={eLive ? 1 : 0.55}
                    class="cursor-pointer transition-colors hover:stroke-destructive {eLive
                      ? 'node-running'
                      : ''}"
                    onclick={() => removeEdge(idx)}
                    role="presentation"
                  />
                {/if}
              {/each}

              <!-- nodes -->
              {#each graph.nodes as n (n.id)}
                {@const k = catalog[n.kind]}
                {@const h = nodeHeight(n)}
                {@const sel = selectedNodeId === n.id}
                {@const live = nodeLive(n.id)}
                <!-- One transform moves the whole node; inner coords are local,
                     so dragging only updates this single attribute (no per-port
                     recompute). -->
                <g transform="translate({n.x} {n.y})" filter={sel || live ? "url(#nodeGlow)" : undefined}>
                  <rect
                    width={NODE_W}
                    height={h}
                    rx="8"
                    stroke-width={sel || live ? 2 : 1}
                    class="fill-card transition-[stroke] {live
                      ? 'stroke-primary'
                      : sel
                        ? 'stroke-primary'
                        : 'stroke-border hover:stroke-primary/50'}"
                  />
                  {#if live}
                    <!-- the executing node: a bright marching-ants emerald border
                         over a breathing solid ring -->
                    <rect
                      width={NODE_W}
                      height={h}
                      rx="8"
                      fill="none"
                      stroke-width="3"
                      class="node-run-glow stroke-primary"
                    />
                    <rect
                      width={NODE_W}
                      height={h}
                      rx="8"
                      fill="none"
                      stroke-width="3"
                      class="node-running stroke-primary"
                    />
                  {/if}
                  <rect
                    width={NODE_W}
                    height={HEAD_H}
                    fill="transparent"
                    class="cursor-move"
                    onpointerdown={(ev) => startDrag(ev, n)}
                    role="presentation"
                  />
                  <circle cx="12" cy={HEAD_H / 2} r="2.5" class="pointer-events-none fill-primary" />
                  <text
                    x="20"
                    y="17"
                    class="pointer-events-none select-none fill-foreground"
                    font-size="11"
                    font-weight="600"
                  >
                    {k?.label ?? n.kind}
                  </text>
                  <line
                    x1="0"
                    y1={HEAD_H}
                    x2={NODE_W}
                    y2={HEAD_H}
                    class="pointer-events-none stroke-border"
                    stroke-width="1"
                  />

                  <!-- input ports -->
                  {#each k?.inputs ?? [] as p, i}
                    {@const wired = wiredInto(n.id, p.name)}
                    <circle
                      cx="0"
                      cy={portY(i)}
                      r="5"
                      fill={wired ? portColor[p.type] : "#151c19"}
                      stroke={portColor[p.type]}
                      stroke-width="2"
                      class="cursor-pointer"
                      onclick={() => clickInPort(n, p.name)}
                      role="presentation"
                    />
                    <text
                      x="11"
                      y={portY(i) + 3}
                      class="pointer-events-none select-none fill-muted-foreground"
                      font-size="10"
                    >
                      {p.name}{p.required ? "*" : ""}
                    </text>
                  {/each}

                  <!-- output ports -->
                  {#each k?.outputs ?? [] as p, i}
                    {@const isPending = pending && pending.node === n.id && pending.port === p.name}
                    <circle
                      cx={NODE_W}
                      cy={portY(i)}
                      r={isPending ? 6 : 5}
                      fill={isPending ? portColor[p.type] : "#151c19"}
                      stroke={portColor[p.type]}
                      stroke-width="2"
                      class="cursor-pointer transition-all"
                      onclick={() => clickOutPort(n, p.name)}
                      role="presentation"
                    />
                    <text
                      x={NODE_W - 11}
                      y={portY(i) + 3}
                      text-anchor="end"
                      class="pointer-events-none select-none fill-muted-foreground"
                      font-size="10"
                    >
                      {p.name}
                    </text>
                  {/each}
                </g>
              {/each}

              <!-- wire being drawn: from the chosen output port to the cursor -->
              {#if pending}
                {@const pn = graph.nodes.find((n) => n.id === pending.node)}
                {#if pn}
                  {@const px = pn.x + NODE_W}
                  {@const py = outPortY(pn, Math.max(0, portIndex(pn, pending.port, "out")))}
                  {@const pColor = portColorOf(pn, pending.port, "out")}
                  <path
                    d={`M ${px} ${py} C ${px + 60} ${py}, ${cursor.x - 60} ${cursor.y}, ${cursor.x} ${cursor.y}`}
                    fill="none"
                    stroke={pColor}
                    stroke-width="2.5"
                    class="node-running pointer-events-none"
                  />
                {/if}
              {/if}
              </g>
            </svg>
          </div>

          <!-- floating controls (over the canvas only) -->
          <!-- top-left: add node -->
          <div class="absolute left-3 top-3">
            <Button
              class="shadow-lg"
              disabled={isRunning}
              onclick={() => {
                nodeSearch = "";
                addOpen = true;
              }}
            >
              <Plus class="size-4" /> Add node
            </Button>
          </div>

          <!-- top-center: transport -->
          <div
            class="absolute left-1/2 top-3 flex -translate-x-1/2 items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
          >
            <Button variant="ghost" size="icon" class="size-8 text-primary hover:text-primary" title="Run now" disabled={running} onclick={() => runNow(graph)}>
              {#if running}<LoaderCircle class="size-4 animate-spin" />{:else}<Play class="size-4" />{/if}
            </Button>
            <Button variant="ghost" size="icon" class="size-8" title="Pause (coming soon)" disabled={!isRunning} onclick={() => comingSoon("Pause")}>
              <Pause class="size-4" />
            </Button>
            <Button variant="ghost" size="icon" class="size-8" title="Stop (coming soon)" disabled={!isRunning} onclick={() => comingSoon("Stop")}>
              <Square class="size-4" />
            </Button>
          </div>

          <!-- top-right: zoom / pan -->
          <div
            class="absolute right-3 top-3 flex items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
          >
            <Button variant="ghost" size="icon" class="size-8" title="Zoom out" onclick={() => zoomBy(1 / 1.2)}><ZoomOut class="size-4" /></Button>
            <span class="w-10 text-center text-[11px] tabular-nums text-muted-foreground">{Math.round(zoom * 100)}%</span>
            <Button variant="ghost" size="icon" class="size-8" title="Zoom in" onclick={() => zoomBy(1.2)}><ZoomIn class="size-4" /></Button>
            <Button variant="ghost" size="icon" class="size-8" title="Fit to view" onclick={fitView}><Maximize class="size-4" /></Button>
          </div>

          <!-- bottom-center: wiring hint -->
          <div class="absolute bottom-3 left-1/2 -translate-x-1/2">
            {#if pending}
              <div class="flex items-center gap-2 rounded-full border border-primary/40 bg-background/85 px-3 py-1 text-xs text-primary shadow-lg backdrop-blur">
                wiring from #{pending.node}.{pending.port} — click an input port
                <button class="text-muted-foreground transition-colors hover:text-foreground" onclick={() => (pending = null)}>cancel</button>
              </div>
            {:else}
              <div class="rounded-full border border-border bg-background/70 px-3 py-1 text-[11px] text-muted-foreground shadow-lg backdrop-blur">
                Click an output ●, then an input ○ to wire · click a wired input to unwire
              </div>
            {/if}
          </div>
        </div>

        <!-- col 2: configuration + node property -->
        <div class="flex basis-1/5 min-w-[14rem] flex-col gap-3 overflow-y-auto">
          <!-- configuration -->
          <div class="space-y-3 rounded-xl border border-border bg-card/40 p-3">
            <div class="flex items-center justify-between gap-2">
              <span class="text-xs font-medium uppercase tracking-wide text-muted-foreground">Configuration</span>
              <Button variant="ghost" size="icon" class="size-7" title="Delete workflow" onclick={() => openDelete(graph)}>
                <Trash2 class="size-3.5" />
              </Button>
            </div>

            <div class="space-y-1.5">
              <Label class="text-xs text-muted-foreground">Name</Label>
              <Input class="h-8" value={graph.name} onchange={(e) => renameInline(e.currentTarget.value)} />
            </div>

            <div class="flex items-center justify-between">
              <Label class="flex items-center gap-1.5 text-sm"><Power class="size-3.5" /> Enabled</Label>
              <Switch checked={graph.enabled} onCheckedChange={(v) => toggleEnabled(graph, v)} />
            </div>

            <div class="space-y-1">
              <div class="flex items-center justify-between">
                <Label class="flex items-center gap-1.5 text-sm"><Unplug class="size-3.5" /> Run orphan nodes</Label>
                <Switch checked={graph.run_orphans ?? false} onCheckedChange={(v) => toggleRunOrphans(graph, v)} />
              </div>
              <p class="text-[10px] leading-snug text-muted-foreground">
                When off, nodes with no connections are skipped during a run.
              </p>
            </div>

            <div class="space-y-1.5">
              <Label class="text-xs text-muted-foreground">Trigger</Label>
              <Select.Root
                type="single"
                value={triggerKind(graph.trigger)}
                onValueChange={(v) => applyTrigger(graph, v ?? "manual", triggerSecs(graph.trigger), triggerLevel(graph.trigger))}
              >
                <Select.Trigger class="h-9 w-full">{labelOf(TRIGGER_OPTS, triggerKind(graph.trigger))}</Select.Trigger>
                <Select.Content>
                  {#each TRIGGER_OPTS as o (o.value)}
                    <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
              {#if triggerKind(graph.trigger) === "cron"}
                <div class="flex items-center gap-2">
                  <Input type="number" min="1" value={triggerSecs(graph.trigger)} class="h-8" onchange={(e) => applyTrigger(graph, "cron", Number(e.currentTarget.value), "info")} />
                  <span class="text-xs text-muted-foreground">sec</span>
                </div>
              {:else if triggerKind(graph.trigger) === "on_new_alert"}
                <Select.Root
                  type="single"
                  value={triggerLevel(graph.trigger)}
                  onValueChange={(v) => applyTrigger(graph, "on_new_alert", 60, v ?? "info")}
                >
                  <Select.Trigger class="h-8 w-full">≥ {triggerLevel(graph.trigger)}</Select.Trigger>
                  <Select.Content>
                    {#each LEVELS as l (l)}<Select.Item value={l} label={`≥ ${l}`}>≥ {l}</Select.Item>{/each}
                  </Select.Content>
                </Select.Root>
              {/if}
            </div>

            <div class="space-y-1.5">
              <Label class="text-xs text-muted-foreground">Concurrency</Label>
              <Select.Root
                type="single"
                value={concKind(graph.concurrency)}
                onValueChange={(v) => applyConcurrency(graph, v ?? "skip", concMax(graph.concurrency))}
              >
                <Select.Trigger class="h-9 w-full">{labelOf(CONC_OPTS, concKind(graph.concurrency))}</Select.Trigger>
                <Select.Content>
                  {#each CONC_OPTS as o (o.value)}
                    <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
              {#if concKind(graph.concurrency) !== "skip"}
                <Input type="number" min="1" value={concMax(graph.concurrency)} class="h-8" onchange={(e) => applyConcurrency(graph, concKind(graph.concurrency), Number(e.currentTarget.value))} />
              {/if}
            </div>
          </div>

          <!-- node property -->
          {#if selectedNode}
            {@const nk = catalog[selectedNode.kind]}
            <div class="space-y-2.5 rounded-xl border border-border bg-card/40 p-3">
              <div class="flex items-center justify-between gap-2">
                <span class="flex min-w-0 items-center gap-2 text-sm font-medium">
                  <span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
                  <span class="truncate">{nk?.label ?? selectedNode.kind}</span>
                  <span class="shrink-0 text-muted-foreground">#{selectedNode.id}</span>
                </span>
                <Button variant="ghost" size="icon" class="size-7 shrink-0" title="Delete node" disabled={isRunning} onclick={() => removeNode(selectedNode)}>
                  <Trash2 class="size-3.5" />
                </Button>
              </div>
              {#if configRows.length === 0}
                <p class="text-xs text-muted-foreground">This node has no settings.</p>
              {:else}
                <div class="space-y-2.5">
                  {#each configRows as { key, port } (key)}
                    {@const wired = wiredInto(selectedNode.id, key)}
                    <div class="space-y-1">
                      <div class="flex items-center justify-between gap-2">
                        <Label class="text-xs text-muted-foreground">
                          {key}{#if port?.required}<span class="text-destructive"> *</span>{/if}
                        </Label>
                        {#if wired}
                          <!-- Link mark: this property is driven by an upstream connection -->
                          <span class="inline-flex items-center gap-1 rounded-full border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-primary">
                            <Link2 class="size-2.5" /> Linked
                          </span>
                        {/if}
                      </div>
                      {#if wired}
                        <div class="rounded-lg border border-primary/30 bg-primary/5 px-2 py-1.5">
                          <div class="flex items-center justify-between gap-2">
                            <p class="flex items-center gap-1.5 text-xs font-medium text-primary">
                              <span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
                              from #{wired.from}.{wired.from_port}
                            </p>
                            <button
                              class="interactive inline-flex items-center gap-1 rounded-md border border-border bg-card px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground hover:text-destructive disabled:pointer-events-none disabled:opacity-50"
                              disabled={isRunning}
                              title="Remove the link and edit this value manually"
                              onclick={() => unlinkPort(selectedNode.id, key)}
                            >
                              <Unplug class="size-2.5" /> Unlink
                            </button>
                          </div>
                          <p class="mt-1 text-[10px] leading-snug text-muted-foreground">
                            Set by the connected node — unlink to fill it manually.
                          </p>
                        </div>
                      {:else if key === "camera"}
                        {#if cameraOptions.length === 0}
                          <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">
                            No cameras — add one in Devices.
                          </p>
                        {:else}
                          {@const cv = resolveOpt(cameraOptions, String(selectedNode.config[key] ?? ""))}
                          <Select.Root type="single" disabled={isRunning} value={cv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? cv })}>
                            <Select.Trigger class="h-8 w-full">{cv}</Select.Trigger>
                            <Select.Content>
                              {#each cameraOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if key === "recipient" || port?.type === "recipient"}
                        {#if recipientOptions.length === 0}
                          <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">
                            No recipients — pair one in Recipients.
                          </p>
                        {:else}
                          {@const rv = resolveOpt(recipientOptions, String(selectedNode.config[key] ?? ""))}
                          <Select.Root type="single" disabled={isRunning} value={rv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? rv })}>
                            <Select.Trigger class="h-8 w-full">{rv}</Select.Trigger>
                            <Select.Content>
                              {#each recipientOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if port?.type === "image"}
                        {@const img = String(selectedNode.config[key] ?? "")}
                        {#if img}
                          <div class="flex items-center gap-2 rounded-lg border border-border bg-card px-2 py-1.5">
                            <ImageIcon class="size-4 shrink-0 text-muted-foreground" />
                            <span class="min-w-0 flex-1 truncate text-xs" title={img}>{fileName(img)}</span>
                            <button
                              class="interactive rounded-md border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground disabled:pointer-events-none disabled:opacity-50"
                              disabled={isRunning}
                              onclick={() => pickImage(selectedNode, key)}
                            >
                              Change
                            </button>
                            <button
                              class="interactive rounded-md border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground hover:text-destructive disabled:pointer-events-none disabled:opacity-50"
                              disabled={isRunning}
                              onclick={() => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: "" })}
                            >
                              Clear
                            </button>
                          </div>
                        {:else}
                          <Button variant="outline" size="sm" class="h-8 w-full justify-start gap-2" disabled={isRunning} onclick={() => pickImage(selectedNode, key)}>
                            <Upload class="size-3.5" /> Upload image…
                          </Button>
                        {/if}
                      {:else if port?.type === "bool"}
                        <div class="flex h-8 items-center gap-2">
                          <Switch
                            checked={selectedNode.config[key] === true || selectedNode.config[key] === "true"}
                            disabled={isRunning}
                            onCheckedChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v })}
                          />
                          <span class="text-xs text-muted-foreground">
                            {selectedNode.config[key] === true || selectedNode.config[key] === "true" ? "True" : "False"}
                          </span>
                        </div>
                      {:else if key === "level"}
                        <Select.Root
                          type="single"
                          disabled={isRunning}
                          value={String(selectedNode.config[key] ?? "info")}
                          onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "info" })}
                        >
                          <Select.Trigger class="h-8 w-full capitalize">{String(selectedNode.config[key] ?? "info")}</Select.Trigger>
                          <Select.Content>
                            {#each LEVELS as l (l)}<Select.Item value={l} label={l} class="capitalize">{l}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else}
                        <Input class="h-8" disabled={isRunning} value={String(selectedNode.config[key] ?? "")} onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })} />
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {:else}
            <div class="rounded-xl border border-dashed border-border p-4 text-center text-xs text-muted-foreground">
              Select a node to edit its properties.
            </div>
          {/if}
        </div>
      </div>

      <!-- row 2: console (collapsed by default) -->
      <div class="shrink-0 overflow-hidden rounded-xl border border-border bg-card/40">
        <button
          class="flex w-full items-center justify-between px-3 py-2 text-sm transition-colors hover:bg-muted/40"
          onclick={() => (consoleOpen = !consoleOpen)}
        >
          <span class="flex items-center gap-2 font-medium">
            <Terminal class="size-4 text-muted-foreground" /> Console
            {#if logs.length}
              <span class="rounded bg-muted px-1.5 text-[10px] tabular-nums text-muted-foreground">{logs.length}</span>
            {/if}
          </span>
          <ChevronDown class="size-4 text-muted-foreground transition-transform {consoleOpen ? '' : '-rotate-90'}" />
        </button>
        {#if consoleOpen}
          <div class="h-[28vh] overflow-y-auto border-t border-border p-3 font-mono text-xs">
            {#if logs.length === 0}
              <p class="text-muted-foreground">No runs yet — run the workflow to see node output here.</p>
            {:else}
              <div class="space-y-0.5">
                {#each logs as log, i (i)}
                  <div class="flex items-center gap-2">
                    <span class="shrink-0 text-muted-foreground">{log.time}</span>
                    <StatusDot state={log.ok ? "online" : "danger"} />
                    <span class="shrink-0 {log.ok ? 'text-primary' : 'text-destructive'}">{log.name}</span>
                    <span class="truncate text-muted-foreground">{log.summary}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- editor view but the graph vanished (deleted elsewhere) -->
  {:else}
    <div class="flex h-full items-center justify-center">
      <EmptyState title="Workflow not found" description="It may have been deleted.">
        <Button onclick={backToList}><ArrowLeft class="size-4" /> Back to workflows</Button>
      </EmptyState>
    </div>
  {/if}
</div>

<!-- Delete dialog -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Delete workflow?</Dialog.Title>
      <Dialog.Description>
        Remove <span class="font-medium text-foreground">{deleteTarget?.name}</span> and its nodes
        and wiring. This can't be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmDelete}>Delete</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Add-node modal — centered over the editor with a subtle backdrop blur -->
{#if addOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
    <button
      class="absolute inset-0 bg-background/40 backdrop-blur-sm duration-200 animate-in fade-in"
      aria-label="Close"
      onclick={() => (addOpen = false)}
    ></button>
    <div
      class="relative z-10 flex max-h-[78vh] w-full max-w-md flex-col rounded-2xl border border-border bg-popover/95 p-4 shadow-2xl duration-200 animate-in fade-in zoom-in-95"
    >
      <div class="mb-3 flex items-center justify-between">
        <h3 class="flex items-center gap-2 text-sm font-semibold">
          <span class="size-1.5 rounded-full bg-primary"></span> Add a node
        </h3>
        <Button variant="ghost" size="icon" class="size-7" onclick={() => (addOpen = false)}>
          <X class="size-4" />
        </Button>
      </div>
      <div class="relative mb-3">
        <Search class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <!-- svelte-ignore a11y_autofocus -->
        <Input
          autofocus
          bind:value={nodeSearch}
          placeholder="Search nodes…"
          class="h-9 pl-8"
        />
      </div>
      <div class="-mr-2 flex-1 space-y-4 overflow-y-auto pr-2">
        {#each nodeCategories as cat (cat.name)}
          <div>
            <div class="mb-1.5 px-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {cat.name}
            </div>
            <div class="space-y-1">
              {#each cat.kinds as k (k.slug)}
                <button
                  class="interactive group flex w-full items-center justify-between gap-3 rounded-xl border border-border bg-card px-3 py-2 text-left"
                  onclick={() => addNodeKind(k.slug)}
                >
                  <span class="flex items-center gap-2 text-sm font-medium">
                    <span class="size-1.5 rounded-full bg-primary transition-transform group-hover:scale-125"></span>
                    {k.label}
                  </span>
                  <span class="shrink-0 text-[10px] tabular-nums text-muted-foreground">
                    {k.inputs.length} in · {k.outputs.length} out
                  </span>
                </button>
              {/each}
            </div>
          </div>
        {:else}
          <div class="flex flex-col items-center gap-2 py-10 text-center">
            <Search class="size-6 text-muted-foreground/50" />
            <p class="text-sm text-muted-foreground">No nodes match “{nodeSearch}”</p>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}
