<script lang="ts">
  import { onMount } from "svelte";
  import { data, onEvent } from "$lib/data";
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
  import PanelRightOpen from "@lucide/svelte/icons/panel-right-open";
  import PanelRightClose from "@lucide/svelte/icons/panel-right-close";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Terminal from "@lucide/svelte/icons/terminal";
  import X from "@lucide/svelte/icons/x";
  import Search from "@lucide/svelte/icons/search";
  import Link2 from "@lucide/svelte/icons/link-2";
  import Unplug from "@lucide/svelte/icons/unplug";
  import Upload from "@lucide/svelte/icons/upload";
  import ImageIcon from "@lucide/svelte/icons/image";
  import FileIcon from "@lucide/svelte/icons/file";
  import Bug from "@lucide/svelte/icons/bug";
  import Printer from "@lucide/svelte/icons/printer";
  import History from "@lucide/svelte/icons/history";
  import DictionaryEditor from "$lib/components/dictionary-editor.svelte";

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
  type PortType =
    | "text" | "num" | "bool" | "image" | "recipient" | "dictionary" | "file" | "exec"
    | "text_array" | "num_array" | "person_array" | "face_array" | "person" | "face" | "any";
  type CatPort = { name: string; type: PortType; required: boolean };
  type CatKind = {
    slug: string;
    label: string;
    category: string;
    inputs: CatPort[];
    outputs: CatPort[];
    default_config: Record<string, unknown>;
  };
  type Node = { id: number; kind: string; x: number; y: number; config: Record<string, unknown>; memoize?: boolean };
  type Edge = { from: number; from_port: string; to: number; to_port: string };
  type Trigger = "Manual" | { Cron: { every_secs: number } };
  type WorkflowEvent = { name: string; category: string; source: string };
  type EventCategory = { name: string; source: string };
  // A palette entry: a node kind, optionally pre-bound to an event name (for the
  // synthesized "On event: Foo" / "Emit event: Foo" entries).
  type PaletteKind = CatKind & { presetEvent?: string };
  type RunRecord = {
    id: string;
    graph_id: number;
    graph_name: string;
    trigger_source: string;
    source_graph_name?: string | null;
    event_name?: string | null;
    pod_name: string;
    status: string;
    result_summary?: string | null;
    started_at: string;
    ended_at?: string | null;
  };
  type RunLog = { node_id?: number | null; label?: string | null; kind: string; message: string };
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
    debug?: boolean;
    nodes: Node[];
    edges: Edge[];
    viewport?: { x: number; y: number; zoom: number };
  };
  type LogLine = {
    time: string;
    name: string;
    ok: boolean;
    summary: string;
    debug?: boolean;
    level?: string;
    ms?: number; // per-node execution time (debug logs only)
  };

  // ---- layout constants ----------------------------------------------------
  const NODE_W = 176;
  const HEAD_H = 26;
  const ROW_H = 22;
  const PAD = 8;
  // ShowImage preview: a 16:9 thumbnail tucked below the node's ports.
  const PREVIEW_W = NODE_W - 16;
  const PREVIEW_H = Math.round((PREVIEW_W * 9) / 16);

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

  // On-screen Print overlay (Unreal-style): transient text flashed over the canvas.
  type ScreenMsg = { id: number; message: string; level: string };
  let screenMsgs = $state<ScreenMsg[]>([]);
  let msgSeq = 0;
  const ON_SCREEN_MS = 4000;
  const screenTimers = new Set<ReturnType<typeof setTimeout>>();
  function flashOnScreen(message: string, level: string) {
    const id = ++msgSeq;
    screenMsgs = [{ id, message, level }, ...screenMsgs];
    const t = setTimeout(() => {
      screenMsgs = screenMsgs.filter((m) => m.id !== id);
      screenTimers.delete(t);
    }, ON_SCREEN_MS);
    screenTimers.add(t);
  }
  // Colour classes for a severity level — kept out of the markup (house style).
  function levelText(level?: string): string {
    if (level === "error") return "text-destructive";
    if (level === "warn") return "text-amber-500";
    if (level === "debug") return "text-muted-foreground";
    return "text-primary"; // info / default
  }
  function logLineClass(log: LogLine): string {
    if (log.level) return levelText(log.level);
    if (log.debug) return "text-muted-foreground";
    return log.ok ? "text-primary" : "text-destructive";
  }
  function screenMsgClass(level: string): string {
    if (level === "error") return "border-destructive/40 text-destructive";
    if (level === "warn") return "border-amber-500/40 text-amber-500";
    if (level === "debug") return "border-border text-muted-foreground";
    return "border-primary/40 text-primary"; // info / default
  }

  // ShowImage debug node: the latest frame that flowed into each node, by node id.
  // Persisted for the session so the preview stays inside the node after a run; an
  // expand button opens it larger in a modal.
  type NodeImage = { src: string; label: string };
  let nodeImages = $state<Record<number, NodeImage>>({});
  let imageModalOpen = $state(false);
  let imageModal = $state<NodeImage | null>(null);
  function openImageModal(img: NodeImage) {
    imageModal = img;
    imageModalOpen = true;
  }

  // resizable right column (config/property panel)
  const PANEL_MIN = 240;
  const PANEL_MAX = 560;
  let panelW = $state(300);
  let panelOpen = $state(true);
  let resizing = $state(false);
  function togglePanel() {
    panelOpen = !panelOpen;
    try {
      localStorage.setItem("wf.panelOpen", String(panelOpen));
    } catch {
      /* ignore */
    }
  }
  function startResize(e: PointerEvent) {
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  }
  function onResizeMove(e: PointerEvent) {
    if (!resizing) return;
    // dragging the splitter left grows the panel (it sits on the panel's left edge)
    const w = window.innerWidth - e.clientX - 16; // 16px = outer page padding
    panelW = Math.max(PANEL_MIN, Math.min(PANEL_MAX, w));
  }
  function endResize(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
    try {
      localStorage.setItem("wf.panelW", String(panelW));
    } catch {
      /* ignore */
    }
  }

  // resizable console (bottom row) — drag the splitter above it to grow/shrink
  const CONSOLE_MIN = 96;
  const CONSOLE_MAX = 720;
  let consoleH = $state(280);
  let resizingConsole = $state(false);
  let consoleDragStartY = 0;
  let consoleDragStartH = 0;
  function startConsoleResize(e: PointerEvent) {
    resizingConsole = true;
    consoleDragStartY = e.clientY;
    consoleDragStartH = consoleH;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  }
  function onConsoleResizeMove(e: PointerEvent) {
    if (!resizingConsole) return;
    // dragging the splitter up grows the console (it sits on the console's top edge)
    const h = consoleDragStartH + (consoleDragStartY - e.clientY);
    consoleH = Math.max(CONSOLE_MIN, Math.min(CONSOLE_MAX, h));
  }
  function endConsoleResize(e: PointerEvent) {
    if (!resizingConsole) return;
    resizingConsole = false;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
    try {
      localStorage.setItem("wf.consoleH", String(consoleH));
    } catch {
      /* ignore */
    }
  }

  // canvas viewport (infinite pan/zoom)
  let pan = $state({ x: 40, y: 40 });
  let zoom = $state(1);
  // cursor in world coords — the live end of a wire being drawn
  let cursor = $state({ x: 0, y: 0 });

  // live references for node dropdowns + per-graph run activity (polled)
  let devices = $state<{ name: string; rtsp_url?: string | null }[]>([]);
  let recipients = $state<{ label: string; enabled: boolean }[]>([]);
  type Activity = {
    running: number;
    current: number | null;
    run_id?: string | null;
    trigger?: string | null;
    pod_name?: string | null;
  };
  let activity = $state<Record<number, Activity>>({});

  // tenant event registry + categories (for the palette + the property combobox)
  let events = $state<WorkflowEvent[]>([]);
  let eventCategories = $state<EventCategory[]>([]);
  // category typed when registering a brand-new event from the property field
  let newEventCategory = $state("");

  // live cursor world position (for the `n`-to-add-here shortcut) + pending drop point
  let hoverWorld = $state<{ x: number; y: number } | null>(null);
  let dropAt: { x: number; y: number } | null = null;

  // recent-runs drawer (per graph in the list)
  let runsOpen = $state(false);
  let runsGraph = $state<Graph | null>(null);
  let runsList = $state<RunRecord[]>([]);
  let runsLogs = $state<Record<string, RunLog[]>>({});
  let runsExpanded = $state<string | null>(null);

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
  const CATEGORY_ORDER = ["Cameras", "Vision", "AI", "Alerts", "Recipients", "Text", "Network", "Flow"];
  // Synthesize per-event palette entries from the tenant registry: each registered event
  // becomes a droppable "On event: <name>" + "Emit event: <name>", grouped under its
  // category (prefixed so the groups sort together). The generic On/Emit event entries
  // stay in "Flow" for authoring a brand-new name.
  const EVENT_GROUP_PREFIX = "Events · ";
  const eventPaletteGroups = $derived.by(() => {
    const onBase = catalog["on_event"];
    const emitBase = catalog["emit_event"];
    if (!onBase || !emitBase) return [] as { name: string; kinds: PaletteKind[] }[];
    const byCat = new Map<string, PaletteKind[]>();
    for (const e of events) {
      const list = byCat.get(e.category) ?? [];
      list.push({ ...onBase, label: `On event: ${e.name}`, presetEvent: e.name });
      list.push({ ...emitBase, label: `Emit event: ${e.name}`, presetEvent: e.name });
      byCat.set(e.category, list);
    }
    return [...byCat.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([cat, kinds]) => ({ name: `${EVENT_GROUP_PREFIX}${cat}`, kinds }));
  });
  const nodeCategories = $derived.by(() => {
    const q = nodeSearch.trim().toLowerCase();
    const matchKind = (k: PaletteKind) =>
      !q ||
      k.label.toLowerCase().includes(q) ||
      k.category.toLowerCase().includes(q) ||
      k.slug.toLowerCase().includes(q);
    const matched = (kinds as PaletteKind[]).filter(matchKind);
    const groups = new Map<string, PaletteKind[]>();
    for (const k of matched) {
      const list = groups.get(k.category) ?? [];
      list.push(k);
      groups.set(k.category, list);
    }
    const ordered: { name: string; kinds: PaletteKind[] }[] = [];
    for (const name of CATEGORY_ORDER) {
      const list = groups.get(name);
      if (list) {
        ordered.push({ name, kinds: list });
        groups.delete(name);
      }
    }
    for (const [name, list] of groups) ordered.push({ name, kinds: list });
    // Event groups last, filtered by the same search query.
    for (const g of eventPaletteGroups) {
      const ks = g.kinds.filter(matchKind);
      if (ks.length) ordered.push({ name: g.name, kinds: ks });
    }
    return ordered;
  });

  // run state for the open graph
  const act = $derived(selectedGraphId != null ? activity[selectedGraphId] : undefined);
  const isRunning = $derived((act?.running ?? 0) > 0);
  // An enabled graph is "armed" (idle, waiting for its trigger) — and a running one
  // is executing. Either way it's ACTIVE, so editing is locked to avoid changing a
  // graph mid-flight; stop it (disable) to edit nodes, configs, or positions.
  const editLocked = $derived(isRunning || (graph?.enabled ?? false));
  // A node (and the edge feeding it) is "live" only while it's the current node.
  function nodeLive(id: number): boolean {
    return liveNode === id;
  }

  // Re-entering the editor mid-run (e.g. during a long Wait) won't get a fresh
  // workflow-node event until that node finishes — so adopt the polled "current"
  // node to resume the live animation where it left off.
  $effect(() => {
    const cur = act?.current ?? null;
    // `!runEnded` guards against the polled `current` lagging behind the run-end
    // event: without it we'd re-light the last node after the flow already ended.
    if (view === "editor" && cur !== null && !runEnded && liveNode === null && liveTimer === null) {
      markLive(cur);
    }
  });

  // option lists for camera / recipient node dropdowns (the real items)
  const cameraOptions = $derived(devices.map((d) => ({ value: d.name, label: d.name })));
  const recipientOptions = $derived(recipients.map((r) => ({ value: r.label, label: r.label })));
  // Resolve a stored value to a real option — falls back to the FIRST item in the
  // list (what "default to first" means) when the stored value is empty/missing.
  function resolveOpt(opts: { value: string; label: string }[], stored: string) {
    return opts.some((o) => o.value === stored) ? stored : (opts[0]?.value ?? "");
  }

  // A graph can only do something when it has at least one wired node — unless it
  // opts into running orphans. With no edges and orphans off, every node is
  // skipped, so a run is a no-op and Run is disabled.
  function canRun(g: Graph): boolean {
    return (g.run_orphans ?? false) || g.edges.length > 0;
  }

  // ---- required-input validation -------------------------------------------
  // A required input is "filled" when it's wired or has a usable config literal.
  function isFilled(node: Node, port: CatPort): boolean {
    if (graph?.edges.some((e) => e.to === node.id && e.to_port === port.name)) return true;
    const v = node.config[port.name];
    switch (port.type) {
      case "bool":
        return typeof v === "boolean";
      case "dictionary":
        return !!v && typeof v === "object" && Object.keys(v).length > 0;
      case "image":
      case "file":
      case "recipient":
        return typeof v === "string" && v.trim() !== "";
      default: // text / num
        return v !== undefined && v !== null && String(v).trim() !== "";
    }
  }
  function requiredUnfilled(node: Node): CatPort[] {
    return (catalog[node.kind]?.inputs ?? []).filter((p) => p.required && !isFilled(node, p));
  }
  function graphHasUnfilled(g: Graph): boolean {
    return g.nodes.some((n) => requiredUnfilled(n).length > 0);
  }
  // The open graph is runnable only when wired up AND every required input is set.
  const graphReady = $derived(!!graph && canRun(graph) && !graphHasUnfilled(graph));

  // ---- render decisions (kept out of the markup) ---------------------------
  // Run-now in the list: only a stopped, runnable, non-cron, complete graph.
  function listRunDisabled(g: Graph): boolean {
    return (
      runStatus(g) !== "disabled" ||
      triggerKind(g.trigger) === "cron" ||
      !canRun(g) ||
      graphHasUnfilled(g)
    );
  }
  function listRunTitle(g: Graph): string {
    const st = runStatus(g);
    if (st === "running") return "Running…";
    if (st === "idle") return "Active — stop it to run manually";
    if (triggerKind(g.trigger) === "cron") return "Runs on a schedule — use the editor to run manually";
    if (graphHasUnfilled(g)) return "A node has a required input not filled";
    if (canRun(g)) return "Run now";
    return "Nothing to run — connect nodes or enable orphans";
  }
  // The emerald/amber/none marching-ants border on a list row.
  function listRowAnim(g: Graph): string {
    const st = runStatus(g);
    if (st === "running") return "list-running";
    if (st === "idle") return "list-idle";
    return "";
  }
  // Run-now in the editor: blocked while busy, active (armed/running), or not ready.
  function editorRunDisabled(): boolean {
    return running || editLocked || !graphReady;
  }
  function editorRunTitle(): string {
    if (!graph) return "";
    if (isRunning) return "Already running";
    if (graph.enabled) return "Graph is active — stop it to run manually";
    if (graphHasUnfilled(graph)) return "A node has a required input not filled";
    if (canRun(graph)) return "Run now";
    return "Nothing to run — connect nodes or enable orphans";
  }
  // A node's border colour: emerald when running/selected, red when incomplete.
  function nodeBorderClass(n: Node): string {
    if (nodeLive(n.id) || selectedNodeId === n.id) return "stroke-primary";
    if (requiredUnfilled(n).length > 0) return "stroke-destructive/70";
    return "stroke-border hover:stroke-primary/50";
  }

  // Property rows = the node's input ports (so wire-only ports like recipient /
  // image are always visible) + the legitimate static settings for that kind.
  // We use the catalog's default_config (the schema) — NOT the node's live
  // config — so stale keys from older schemas (e.g. notify_beacon's `target`)
  // are never rendered.
  const configRows = $derived.by(() => {
    if (!selectedNode) return [] as { key: string; port: CatPort | null }[];
    const nk = catalog[selectedNode.kind];
    // Exec pins are pure wiring — never editable settings, so exclude them.
    const inputs = (nk?.inputs ?? []).filter((p) => p.type !== "exec");
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

  // Control-flow (exec) ports are white. Every node has one `exec_in` pin on the
  // left header; exec OUTPUTS (usually `exec_out`, but `true`/`false` for If) stack
  // down the right edge from the header.
  const EXEC_IN_NAME = "exec_in";
  function portOf(node: Node, port: string, side: "in" | "out"): CatPort | undefined {
    const list = side === "out" ? catalog[node.kind]?.outputs : catalog[node.kind]?.inputs;
    return list?.find((p) => p.name === port);
  }
  function isExec(node: Node, port: string, side: "in" | "out"): boolean {
    return portOf(node, port, side)?.type === "exec";
  }
  // Data ports = everything except the white exec pins.
  function dataPorts(k: CatKind | undefined, side: "in" | "out"): CatPort[] {
    const list = side === "in" ? k?.inputs : k?.outputs;
    return (list ?? []).filter((p) => p.type !== "exec");
  }
  function execOutPorts(k: CatKind | undefined): CatPort[] {
    return (k?.outputs ?? []).filter((p) => p.type === "exec");
  }

  // Output data ports stack BELOW the exec-out pins (which occupy the first rows from the
  // header). Without this a node with >1 exec out (For each: loop_body/completed) overlaps
  // its data outputs (item/index). Single-exec nodes get offset 0 (unchanged).
  function outDataOffset(k: CatKind | undefined): number {
    return Math.max(0, execOutPorts(k).length - 1);
  }
  // Y where a node's body ends and the optional ShowImage preview begins.
  function portsBottom(n: Node): number {
    const k = catalog[n.kind];
    const execCount = execOutPorts(k).length;
    const outRows = dataPorts(k, "out").length + outDataOffset(k);
    const rows = Math.max(dataPorts(k, "in").length, outRows, execCount, 1);
    return HEAD_H + rows * ROW_H;
  }
  // A ShowImage node grows to fit its preview thumbnail once a frame has arrived.
  function hasPreview(n: Node): boolean {
    return n.kind === "show_image" && !!nodeImages[n.id];
  }
  function nodeHeight(n: Node): number {
    return portsBottom(n) + PAD + (hasPreview(n) ? PREVIEW_H + PAD : 0);
  }
  // Local Y of the i-th data port within a node (the <g> is translated to n.x/n.y).
  function dataPortY(i: number) {
    return HEAD_H + i * ROW_H + ROW_H / 2;
  }
  // Local Y of a port by name — exec_in on the header; exec outputs stack from the
  // header down; data ports sit below the header divider.
  function portLocalY(node: Node, port: string, side: "in" | "out") {
    if (isExec(node, port, side)) {
      if (side === "in") return HEAD_H / 2;
      const i = execOutPorts(catalog[node.kind]).findIndex((p) => p.name === port);
      return HEAD_H / 2 + Math.max(0, i) * ROW_H;
    }
    const i = dataPorts(catalog[node.kind], side).findIndex((p) => p.name === port);
    const off = side === "out" ? outDataOffset(catalog[node.kind]) : 0;
    return dataPortY(Math.max(0, i) + off);
  }
  // Absolute port Y (for drawing edges between nodes).
  function inPortY(n: Node, port: string) {
    return n.y + portLocalY(n, port, "in");
  }
  function outPortY(n: Node, port: string) {
    return n.y + portLocalY(n, port, "out");
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
    dictionary: "#2dd4bf",
    file: "#fb923c",
    exec: "#f4f4f5",
    // Typed arrays (brighter shades of their element color) + detection elements + wildcard.
    text_array: "#93c5fd",
    num_array: "#6ee7b7",
    person_array: "#f0abfc",
    face_array: "#fda4af",
    person: "#e879f9",
    face: "#fb7185",
    any: "#94a3b8",
  };
  // The element colour for an array type (so a ForEach `item` mirrors the wired array).
  function arrayElementColor(t: PortType): string {
    switch (t) {
      case "person_array": return portColor.person;
      case "face_array": return portColor.face;
      case "num_array": return portColor.num;
      case "text_array": return portColor.text;
      default: return portColor[t] ?? portColor.any; // already an element type, else wildcard
    }
  }
  // The colour of a port (so a connection follows its data type). Special case: a ForEach
  // `item` output takes the element colour of whatever array is wired into `items` — e.g. a
  // pink PersonDetection[] in → a pink `item` (and pink outgoing edges).
  function portColorOf(node: Node, port: string, side: "in" | "out"): string {
    if (node.kind === "for_each" && side === "out" && port === "item") {
      const e = wiredInto(node.id, "items");
      const src = e ? graph?.nodes.find((n) => n.id === e.from) : undefined;
      const srcType = src && e ? portOf(src, e.from_port, "out")?.type : undefined;
      return srcType ? arrayElementColor(srcType) : portColor.any;
    }
    const p = portOf(node, port, side);
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
    data.setGraphViewport(id, v.x, v.y, v.zoom).catch(() => {});
  }
  function openGraph(id: number) {
    saveViewport();
    selectedGraphId = id;
    selectedNodeId = null;
    pending = null;
    nodeImages = {}; // node ids restart per graph — drop the previous graph's previews
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
    graphs = await data.loadGraphs<Graph>();
    if (selectedGraphId !== null && !graphs.some((g) => g.id === selectedGraphId)) {
      selectedGraphId = null;
    }
  }

  async function loadRefs() {
    try {
      const [d, r] = await Promise.all([data.listDevices(), data.listBeacons()]);
      devices = d;
      recipients = r;
    } catch (err) {
      console.error("Failed to load devices/recipients", err);
    }
  }
  async function loadEvents() {
    try {
      const [ev, cats] = await Promise.all([
        data.listEvents<WorkflowEvent[]>(),
        data.listEventCategories<EventCategory[]>(),
      ]);
      events = ev ?? [];
      eventCategories = cats ?? [];
    } catch (err) {
      console.error("Failed to load events", err);
    }
  }

  onMount(() => {
    const savedW = Number(localStorage.getItem("wf.panelW"));
    if (savedW >= PANEL_MIN && savedW <= PANEL_MAX) panelW = savedW;
    const savedOpen = localStorage.getItem("wf.panelOpen");
    if (savedOpen != null) panelOpen = savedOpen === "true";
    const savedConsoleH = Number(localStorage.getItem("wf.consoleH"));
    if (savedConsoleH >= CONSOLE_MIN && savedConsoleH <= CONSOLE_MAX) consoleH = savedConsoleH;
    (async () => {
      try {
        const cat = await data.nodeCatalog<{ kinds: CatKind[] }>();
        catalog = Object.fromEntries(cat.kinds.map((k) => [k.slug, k]));
        await reload();
        await loadRefs();
        await loadEvents();
      } catch (err) {
        toast.error("Failed to load workflows", { description: String(err) });
      } finally {
        loading = false;
      }
    })();
    const un = onEvent("workflow-run", (payload) => {
      running = false;
      const { graph: gid, name, ok, summary } = payload as {
        graph: number;
        name: string;
        ok: boolean;
        summary: string;
      };
      // Run finished (ok or error) → drain remaining highlights, then stop.
      if (gid === selectedGraphId) endLive();
      logs = [{ time: new Date().toLocaleTimeString(), name, ok, summary }, ...logs].slice(0, 200);
      if (ok) toast.success(`Workflow "${name}"`, { description: summary });
      else toast.error(`Workflow "${name}" failed`, { description: summary });
    });
    // Per-node execution events (real-time, catch every node).
    const unNode = onEvent("workflow-node", (payload) => {
      const p = payload as { graph: number; node: number };
      if (p.graph === selectedGraphId) markLive(p.node);
    });
    // Per-node debug logs (only emitted when the graph's debug flag is on).
    const unLog = onEvent("workflow-log", (payload) => {
      const p = payload as { graph: number; node: number; label: string; message: string; ms: number };
      if (p.graph !== selectedGraphId) return; // only the open graph
      const { label, message, ms } = p;
      logs = [
        { time: new Date().toLocaleTimeString(), name: label, ok: true, summary: message, debug: true, ms },
        ...logs,
      ].slice(0, 200);
    });
    // Print node: always echo to the Console (independent of the debug flag) and,
    // when on_screen, flash it over the canvas. Not graph-filtered — the payload
    // carries no graph id (see plan); the editor shows one graph at a time.
    const unPrint = onEvent("workflow-print", (payload) => {
      const { message, level, on_screen } = payload as { message: string; level: string; on_screen: boolean };
      logs = [
        { time: new Date().toLocaleTimeString(), name: "Print", ok: true, summary: message, level },
        ...logs,
      ].slice(0, 200);
      if (on_screen) flashOnScreen(message, level);
    });
    // Show image node: stash the frame inside that node (debug-only, by node id).
    const unImage = onEvent("workflow-image", (payload) => {
      const { graph: gid, node, label, data: frame } = payload as {
        graph: number;
        node: number;
        label: string;
        data: string;
      };
      if (gid !== selectedGraphId) return; // node ids are per-graph — only the open one
      nodeImages = { ...nodeImages, [node]: { src: `data:image/jpeg;base64,${frame}`, label } };
    });
    // Cross-graph events: dispatched on the emitter, triggered/skipped on each listener.
    // Show the line in whichever graph's run view is open (matched by `graph`).
    const unEvent = onEvent("workflow-event", (payload) => {
      const p = payload as {
        graph: number | null;
        event: string;
        kind?: string;
        by_graph?: string;
        count?: number;
        reason?: string;
      };
      if (p.graph !== selectedGraphId) return;
      const summary = eventLogSummary(p);
      if (!summary) return;
      logs = [
        { time: new Date().toLocaleTimeString(), name: "Event", ok: p.kind !== "skip", summary, level: p.kind === "skip" ? "warn" : "info" },
        ...logs,
      ].slice(0, 200);
    });
    // Poll run activity (running graphs + status) for the lock + list badges.
    const poll = setInterval(async () => {
      try {
        activity = await data.runStates<Record<number, Activity>>();
      } catch {
        /* ignore transient errors */
      }
    }, 700);
    return () => {
      saveViewport(); // persist the viewport when leaving the page too
      un.then((u) => u());
      unNode.then((u) => u());
      unLog.then((u) => u());
      unPrint.then((u) => u());
      unImage.then((u) => u());
      unEvent.then((u) => u());
      screenTimers.forEach((t) => clearTimeout(t));
      screenTimers.clear();
      clearInterval(poll);
    };
  });

  // A console line for a cross-graph event signal.
  function eventLogSummary(p: { event: string; kind?: string; by_graph?: string; count?: number; reason?: string }): string {
    if (p.kind === "dispatch") return `Dispatched '${p.event}' to ${p.count ?? 0} listener(s)`;
    if (p.kind === "trigger") return `Event '${p.event}' triggered by ${p.by_graph ?? "another graph"}`;
    if (p.kind === "skip") {
      return p.reason === "capacity"
        ? `Event '${p.event}' skipped — pod at capacity`
        : `Event '${p.event}' skipped — handler already running`;
    }
    return `Event '${p.event}'`;
  }

  // ---- graph CRUD ----------------------------------------------------------
  async function createWorkflow() {
    try {
      const id = await data.createGraph("Untitled");
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
      await data.renameGraph(graph.id, n);
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
      await data.deleteGraph(deleteTarget.id);
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
      await data.setGraphEnabled(g.id, enabled);
      await reload();
    } catch (err) {
      g.enabled = !enabled; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function toggleRunOrphans(g: Graph, run_orphans: boolean) {
    g.run_orphans = run_orphans; // optimistic
    try {
      await data.setGraphRunOrphans(g.id, run_orphans);
      await reload();
    } catch (err) {
      g.run_orphans = !run_orphans; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function toggleDebug(g: Graph, debug: boolean) {
    g.debug = debug; // optimistic
    try {
      await data.setGraphDebug(g.id, debug);
      await reload();
    } catch (err) {
      g.debug = !debug; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function runNow(g: Graph) {
    running = true;
    try {
      const msg = await data.runGraph(g.id);
      toast.info(msg);
    } catch (err) {
      running = false;
      toast.error("Run", { description: String(err) });
    }
  }

  // ---- trigger / concurrency ----------------------------------------------
  function triggerKind(t: Trigger): "manual" | "cron" {
    if (t === "Manual") return "manual";
    return "cron";
  }
  function triggerSecs(t: Trigger): number {
    return typeof t === "object" && "Cron" in t ? t.Cron.every_secs : 60;
  }
  function triggerSummary(t: Trigger): string {
    if (t === "Manual") return "Manual";
    return `Every ${t.Cron.every_secs}s`;
  }
  function concKind(c: Concurrency): "skip" | "queue" | "concurrent" {
    if (c === "SkipIfRunning") return "skip";
    return "QueueSingle" in c ? "queue" : "concurrent";
  }
  function concMax(c: Concurrency): number {
    if (typeof c !== "object") return 1;
    return "QueueSingle" in c ? c.QueueSingle.max_queued : c.Concurrent.max_concurrent;
  }
  async function applyTrigger(g: Graph, kind: string, everySecs: number) {
    await data.setGraphTrigger(g.id, kind, everySecs);
    await reload();
  }
  async function applyConcurrency(g: Graph, kind: string, max: number) {
    await data.setGraphConcurrency(g.id, kind, max);
    await reload();
  }

  // ---- node + edge ops -----------------------------------------------------
  async function addNodeKind(slug: string, presetEvent?: string) {
    if (!graph || editLocked) return;
    // Place at the cursor (when opened via the `n` shortcut), else cascade.
    let x: number;
    let y: number;
    if (dropAt) {
      x = Math.round(dropAt.x);
      y = Math.round(dropAt.y);
      dropAt = null;
    } else {
      const n = graph.nodes.length;
      x = 80 + (n % 5) * 48;
      y = 70 + (n % 5) * 40;
    }
    const id = await data.addNode(graph.id, slug, x, y);
    // Default camera/recipient configs to the actual first item in each list; for a
    // synthesized event entry, pre-bind its event name.
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
    if (presetEvent) {
      cfg.event = presetEvent;
      patched = true;
    }
    if (patched) await data.setNodeConfig(graph.id, id, cfg);
    selectedNodeId = id;
    addOpen = false;
    await reload();
  }

  // Press `n` over the canvas → open the palette and drop the next node at the cursor.
  // Ignored while typing in a field or while the graph is active (editLocked).
  function onWindowKey(e: KeyboardEvent) {
    if (e.key !== "n" && e.key !== "N") return;
    if (view !== "editor" || editLocked || addOpen) return;
    const el = document.activeElement as HTMLElement | null;
    const tag = el?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el?.isContentEditable) return;
    e.preventDefault();
    dropAt = hoverWorld;
    nodeSearch = "";
    addOpen = true;
  }

  // Unreal-Blueprint-style header tint by category, so node families read at a glance and
  // events POP. Returns a hex colour; the header band is drawn at low alpha behind the label.
  function headerColor(kind: string): string {
    if (kind === "emit_event" || kind === "on_event") return "#f43f5e"; // event red — stands out
    switch (catalog[kind]?.category) {
      case "Flow": return "#94a3b8"; // slate
      case "Vision": return portColor.person; // violet/magenta
      case "AI": return portColor.dictionary; // teal
      case "Alerts": return portColor.bool; // amber
      case "Cameras": return portColor.text; // blue
      case "Recipients": return portColor.recipient; // purple
      case "Text": return portColor.num; // green
      case "Network": return "#22d3ee"; // cyan
      default: return "#64748b";
    }
  }

  // Register the (already-saved) event name into the tenant registry under the typed
  // category (default "General") — so other graphs can listen for it and it appears in the
  // palette grouped by category. Explicit (a button), not coupled to typing the name.
  async function registerEvent(name: string) {
    const ev = name.trim();
    if (!ev) return;
    try {
      await data.createEvent(ev, newEventCategory.trim() || "General");
      await loadEvents();
      newEventCategory = "";
      toast.success(`Event “${ev}” registered`);
    } catch (err) {
      toast.error("Couldn't register event", { description: String(err) });
    }
  }

  // ---- recent-runs drawer (list view) --------------------------------------
  async function openRuns(g: Graph) {
    runsGraph = g;
    runsOpen = true;
    runsList = [];
    runsLogs = {};
    runsExpanded = null;
    try {
      runsList = await data.runHistory<RunRecord[]>(g.id, 50);
    } catch (err) {
      toast.error("Couldn't load run history", { description: String(err) });
    }
  }
  async function toggleRunLogs(runId: string) {
    if (runsExpanded === runId) {
      runsExpanded = null;
      return;
    }
    runsExpanded = runId;
    if (!runsLogs[runId]) {
      try {
        runsLogs = { ...runsLogs, [runId]: await data.runLogs<RunLog[]>(runId) };
      } catch (err) {
        toast.error("Couldn't load run logs", { description: String(err) });
      }
    }
  }
  function runStatusClass(status: string): string {
    if (status === "ok") return "text-primary";
    if (status === "failed") return "text-destructive";
    return "text-amber-500"; // running
  }
  function triggerSourceLabel(src?: string | null): string {
    if (src === "cron") return "Cron";
    if (src === "event") return "Event";
    if (src === "manual") return "Manual";
    return "Running";
  }

  function comingSoon(what: string) {
    toast.info(`${what} — coming soon`);
  }
  async function removeNode(n: Node) {
    if (!graph) return;
    await data.removeNode(graph.id, n.id);
    if (selectedNodeId === n.id) selectedNodeId = null;
    if (nodeImages[n.id]) {
      const { [n.id]: _drop, ...rest } = nodeImages;
      nodeImages = rest;
    }
    await reload();
  }
  async function removeEdge(idx: number) {
    if (!graph || editLocked) return;
    await data.removeEdge(graph.id, idx);
    await reload();
  }
  // Drop the wire feeding a node's input port so the user can fill it manually.
  async function unlinkPort(nodeId: number, port: string) {
    if (!graph || editLocked) return;
    const idx = graph.edges.findIndex((e) => e.to === nodeId && e.to_port === port);
    if (idx >= 0) await removeEdge(idx);
  }
  function clickOutPort(node: Node, port: string) {
    if (editLocked) return;
    pending = { node: node.id, port };
    // seed the wire's free end at the port so it starts with zero length
    cursor = { x: node.x + NODE_W, y: outPortY(node, port) };
  }
  async function clickInPort(node: Node, port: string) {
    if (!graph || editLocked) return;
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
      await data.addEdge(graph.id, pending.node, pending.port, node.id, port);
      pending = null;
      await reload();
    } catch (err) {
      toast.error("Can't wire that", { description: String(err) });
    }
  }
  async function saveNodeConfig(n: Node, config: Record<string, unknown>) {
    if (!graph) return;
    await data.setNodeConfig(graph.id, n.id, config);
    await reload();
  }
  async function saveNodeMemoize(n: Node, memoize: boolean) {
    if (!graph) return;
    await data.setNodeMemoize(graph.id, n.id, memoize);
    await reload();
  }
  // Pick a file via a hidden browser input. Browsers don't expose real filesystem paths,
  // so we store the file name as the literal — the edge/pod resolves the actual path/URL
  // at run time. (You can also type a path/URL directly into the field.)
  function pickViaInput(accept: string): Promise<string | null> {
    return new Promise((resolve) => {
      const input = document.createElement("input");
      input.type = "file";
      if (accept) input.accept = accept;
      input.onchange = () => resolve(input.files?.[0]?.name ?? null);
      input.click();
    });
  }
  async function pickImage(n: Node, key: string) {
    if (editLocked) return;
    const picked = await pickViaInput("image/*");
    if (picked) await saveNodeConfig(n, { ...n.config, [key]: picked });
  }
  async function pickFile(n: Node, key: string) {
    if (editLocked) return;
    const picked = await pickViaInput("");
    if (picked) await saveNodeConfig(n, { ...n.config, [key]: picked });
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
    if (editLocked) return; // …but no moving while the graph is active (running/armed)
    const wp = worldPoint(e);
    drag = { id: n.id, offX: wp.x - n.x, offY: wp.y - n.y };
  }
  function onCanvasDown(e: PointerEvent) {
    if (e.target !== e.currentTarget) return; // only grab empty canvas, not a node
    if (pending) {
      pending = null; // clicking empty space cancels the wire being drawn
      return;
    }
    selectedNodeId = null; // clicking the empty canvas deselects the node
    panning = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
    svgEl?.setPointerCapture?.(e.pointerId);
  }
  function onCanvasMove(e: PointerEvent) {
    hoverWorld = worldPoint(e); // track the cursor for the `n`-to-add-here shortcut
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
      if (n) await data.moveNode(graph.id, id, n.x, n.y);
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

<svelte:window onkeydown={onWindowKey} />

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
                    class="cursor-pointer transition-colors hover:bg-primary/5 {listRowAnim(g)}"
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
                        <div class="flex flex-col gap-0.5">
                          <span
                            class="inline-flex w-fit items-center gap-1.5 rounded-full border border-primary/30 bg-primary/10 px-2 py-0.5 text-[11px] font-medium text-primary"
                          >
                            <LoaderCircle class="size-3 animate-spin" />
                            {#if activity[g.id]?.trigger}{triggerSourceLabel(activity[g.id]?.trigger)}{:else}Running{/if}
                          </span>
                          {#if activity[g.id]?.pod_name}
                            <span class="text-[10px] text-muted-foreground">on {activity[g.id]?.pod_name}</span>
                          {/if}
                        </div>
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
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-8 text-primary"
                          title={listRunTitle(g)}
                          disabled={listRunDisabled(g)}
                          onclick={() => runNow(g)}
                        >
                          {#if st === "running"}
                            <LoaderCircle class="size-4 animate-spin" />
                          {:else}
                            <Play class="size-4" />
                          {/if}
                        </Button>
                        <Button variant="ghost" size="icon" class="size-8" title="Recent runs" onclick={() => openRuns(g)}>
                          <History class="size-4" />
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
        {:else if editLocked}
          <span
            class="flex items-center gap-1.5 rounded-full border border-amber-500/40 bg-amber-500/10 px-2.5 py-0.5 text-[11px] font-medium text-amber-500"
          >
            <span class="size-1.5 rounded-full bg-amber-500 status-live"></span>
            Active — stop to edit
          </span>
        {/if}
      </div>

      <!-- row 1: editor + resizable side panel -->
      <div class="flex min-h-0 flex-1 gap-1">
        <!-- col 1: the node editor -->
        <div class="relative min-w-0 flex-1">
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
                  {@const y1 = outPortY(fromN, e.from_port)}
                  {@const x2 = toN.x}
                  {@const y2 = inPortY(toN, e.to_port)}
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
                {@const invalid = requiredUnfilled(n).length > 0}
                <!-- One transform moves the whole node; inner coords are local,
                     so dragging only updates this single attribute (no per-port
                     recompute). -->
                <g transform="translate({n.x} {n.y})" filter={sel || live ? "url(#nodeGlow)" : undefined}>
                  <rect
                    width={NODE_W}
                    height={h}
                    rx="8"
                    stroke-width={sel || live || invalid ? 2 : 1}
                    class="fill-card transition-[stroke] {nodeBorderClass(n)}"
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
                  <!-- Category-tinted header band (Unreal-Blueprint style): a top-rounded
                       band behind the label so node families read at a glance and events pop.
                       Drawn before the drag handle so the handle stays on top for clicks. -->
                  <path
                    d="M0 8 a8 8 0 0 1 8 -8 h{NODE_W - 16} a8 8 0 0 1 8 8 v{HEAD_H - 8} h-{NODE_W} z"
                    fill={headerColor(n.kind)}
                    fill-opacity={n.kind === "emit_event" || n.kind === "on_event" ? 0.32 : 0.18}
                    class="pointer-events-none"
                  />
                  <!-- Drag handle covers the whole node body; ports render after
                       this rect, so they stay clickable on top of it. -->
                  <rect
                    width={NODE_W}
                    height={h}
                    rx="8"
                    fill="transparent"
                    class="cursor-move"
                    onpointerdown={(ev) => startDrag(ev, n)}
                    role="presentation"
                  />
                  <text
                    x="14"
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

                  {#if invalid}
                    <!-- required input(s) not filled — a red dot, the logo's signal -->
                    <circle cx={NODE_W - 9} cy="9" r="5.5" class="pointer-events-none fill-destructive/20" />
                    <circle cx={NODE_W - 9} cy="9" r="3" class="fill-destructive">
                      <title>Required input not filled: {requiredUnfilled(n).map((p) => p.name).join(", ")}</title>
                    </circle>
                  {/if}

                  <!-- exec (control-flow) pins — white. exec_in on the left header. -->
                  <circle
                    cx="0"
                    cy={HEAD_H / 2}
                    r="5"
                    fill={wiredInto(n.id, EXEC_IN_NAME) ? portColor.exec : "#151c19"}
                    stroke={portColor.exec}
                    stroke-width="2"
                    class="cursor-pointer"
                    onclick={() => clickInPort(n, EXEC_IN_NAME)}
                    role="presentation"
                  />
                  <!-- exec outputs stack down the right edge (exec_out, or true/false) -->
                  {#each execOutPorts(k) as p, i}
                    {@const ey = HEAD_H / 2 + i * ROW_H}
                    {@const isP = pending && pending.node === n.id && pending.port === p.name}
                    <circle
                      cx={NODE_W}
                      cy={ey}
                      r={isP ? 6 : 5}
                      fill={isP ? portColor.exec : "#151c19"}
                      stroke={portColor.exec}
                      stroke-width="2"
                      class="cursor-pointer transition-all"
                      onclick={() => clickOutPort(n, p.name)}
                      role="presentation"
                    />
                    {#if execOutPorts(k).length > 1}
                      <text
                        x={NODE_W - 9}
                        y={ey + 3}
                        text-anchor="end"
                        class="pointer-events-none select-none fill-muted-foreground"
                        font-size="9"
                      >
                        {p.name}
                      </text>
                    {/if}
                  {/each}

                  <!-- input data ports -->
                  {#each dataPorts(k, "in") as p, i}
                    {@const wired = wiredInto(n.id, p.name)}
                    {@const need = p.required && !isFilled(n, p)}
                    <circle
                      cx="0"
                      cy={dataPortY(i)}
                      r="5"
                      fill={wired ? portColor[p.type] : "#151c19"}
                      stroke={need ? "#ef4444" : portColor[p.type]}
                      stroke-width="2"
                      class="cursor-pointer"
                      onclick={() => clickInPort(n, p.name)}
                      role="presentation"
                    />
                    <text
                      x="11"
                      y={dataPortY(i) + 3}
                      class="pointer-events-none select-none {need ? 'fill-destructive' : 'fill-muted-foreground'}"
                      font-size="10"
                    >
                      {p.name}{p.required ? "*" : ""}
                    </text>
                  {/each}

                  <!-- output data ports (stacked below the exec-out pins) -->
                  {#each dataPorts(k, "out") as p, i}
                    {@const isPending = pending && pending.node === n.id && pending.port === p.name}
                    {@const dy = dataPortY(i + outDataOffset(k))}
                    {@const pc = portColorOf(n, p.name, "out")}
                    <circle
                      cx={NODE_W}
                      cy={dy}
                      r={isPending ? 6 : 5}
                      fill={isPending ? pc : "#151c19"}
                      stroke={pc}
                      stroke-width="2"
                      class="cursor-pointer transition-all"
                      onclick={() => clickOutPort(n, p.name)}
                      role="presentation"
                    />
                    <text
                      x={NODE_W - 11}
                      y={dy + 3}
                      text-anchor="end"
                      class="pointer-events-none select-none fill-muted-foreground"
                      font-size="10"
                    >
                      {p.name}
                    </text>
                  {/each}

                  <!-- ShowImage preview: the frame that passed through, drawn as a
                       native SVG <image> so it honours this node's transform (a
                       <foreignObject> would ignore it in WebKit and float to 0,0).
                       Drag the thumbnail to move the node; the corner button enlarges. -->
                  {#if hasPreview(n)}
                    {@const pv = nodeImages[n.id]}
                    {@const ix = 8}
                    {@const iy = portsBottom(n) + 2}
                    <clipPath id="preview-clip-{n.id}">
                      <rect x={ix} y={iy} width={PREVIEW_W} height={PREVIEW_H} rx="6" />
                    </clipPath>
                    <image
                      href={pv.src}
                      x={ix}
                      y={iy}
                      width={PREVIEW_W}
                      height={PREVIEW_H}
                      preserveAspectRatio="xMidYMid slice"
                      clip-path="url(#preview-clip-{n.id})"
                      class="cursor-move"
                      onpointerdown={(ev) => startDrag(ev, n)}
                      role="presentation"
                    />
                    <rect
                      x={ix}
                      y={iy}
                      width={PREVIEW_W}
                      height={PREVIEW_H}
                      rx="6"
                      fill="none"
                      class="pointer-events-none stroke-border"
                      stroke-width="1"
                    />
                    <!-- enlarge button (top-right corner of the thumbnail) -->
                    <g
                      transform="translate({ix + PREVIEW_W - 22} {iy + 4})"
                      class="cursor-pointer"
                      onclick={(e) => {
                        e.stopPropagation();
                        openImageModal(pv);
                      }}
                      role="presentation"
                    >
                      <title>Open larger</title>
                      <rect width="18" height="18" rx="4" class="fill-background stroke-border" stroke-width="1" />
                      <g
                        transform="translate(3 3) scale(0.5)"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        class="pointer-events-none text-muted-foreground"
                      >
                        <path d="M8 3H5a2 2 0 0 0-2 2v3" />
                        <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
                        <path d="M3 16v3a2 2 0 0 0 2 2h3" />
                        <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
                      </g>
                    </g>
                  {/if}
                </g>
              {/each}

              <!-- wire being drawn: from the chosen output port to the cursor -->
              {#if pending}
                {@const pn = graph.nodes.find((n) => n.id === pending.node)}
                {#if pn}
                  {@const px = pn.x + NODE_W}
                  {@const py = outPortY(pn, pending.port)}
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
              disabled={editLocked}
              onclick={() => {
                nodeSearch = "";
                addOpen = true;
              }}
            >
              <Plus class="size-4" /> Add node
            </Button>
          </div>

          <!-- on-screen Print messages (Unreal-style debug text, newest on top) -->
          {#if screenMsgs.length}
            <div class="pointer-events-none absolute left-3 top-16 z-10 flex max-w-sm flex-col gap-1.5">
              {#each screenMsgs as msg (msg.id)}
                <div
                  class="flex items-center gap-2 rounded-lg border bg-background/85 px-3 py-1.5 font-mono text-xs shadow-lg backdrop-blur duration-200 animate-in fade-in slide-in-from-left-2 {screenMsgClass(msg.level)}"
                >
                  <Printer class="size-3 shrink-0" />
                  <span class="truncate">{msg.message}</span>
                </div>
              {/each}
            </div>
          {/if}

          <!-- top-center: transport -->
          <div
            class="absolute left-1/2 top-3 flex -translate-x-1/2 items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
          >
            <Button
              variant="ghost"
              size="icon"
              class="size-8 text-primary hover:text-primary"
              title={editorRunTitle()}
              disabled={editorRunDisabled()}
              onclick={() => runNow(graph)}
            >
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

        {#if !panelOpen}
          <!-- collapsed: thin rail to reopen the panel -->
          <button
            class="interactive group flex w-7 shrink-0 items-center justify-center rounded-xl border border-border bg-card/40 hover:bg-muted/40"
            title="Show panel"
            aria-label="Show panel"
            onclick={togglePanel}
          >
            <PanelRightOpen class="size-4 text-muted-foreground group-hover:text-foreground" />
          </button>
        {:else}
        <!-- splitter: drag to resize the side panel -->
        <div
          class="group relative flex w-2 shrink-0 cursor-col-resize items-center justify-center"
          onpointerdown={startResize}
          onpointermove={onResizeMove}
          onpointerup={endResize}
          onpointercancel={endResize}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize panel"
        >
          <div
            class="h-10 w-1 rounded-full transition-colors {resizing
              ? 'bg-primary'
              : 'bg-border group-hover:bg-primary/60'}"
          ></div>
        </div>

        <!-- col 2: configuration + node property -->
        <div
          class="flex shrink-0 flex-col gap-3 overflow-y-auto"
          style="width: {panelW}px"
        >
          <!-- configuration -->
          <div class="space-y-3 rounded-xl border border-border bg-card/40 p-3">
            <div class="flex items-center justify-between gap-2">
              <span class="text-xs font-medium uppercase tracking-wide text-muted-foreground">Configuration</span>
              <div class="flex items-center gap-1">
                <Button variant="ghost" size="icon" class="size-7" title="Delete workflow" onclick={() => openDelete(graph)}>
                  <Trash2 class="size-3.5" />
                </Button>
                <Button variant="ghost" size="icon" class="size-7" title="Hide panel" onclick={togglePanel}>
                  <PanelRightClose class="size-3.5" />
                </Button>
              </div>
            </div>

            <!-- Name -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3">
              <Label class="text-xs text-muted-foreground">Name</Label>
              <Input class="h-8" disabled={editLocked} value={graph.name} onchange={(e) => renameInline(e.currentTarget.value)} />
            </div>

            <!-- Enabled -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3">
              <Label class="flex items-center gap-1.5 text-xs text-muted-foreground"><Power class="size-3.5" /> Enabled</Label>
              <div class="flex h-8 items-center">
                <Switch checked={graph.enabled} onCheckedChange={(v) => toggleEnabled(graph, v)} />
              </div>
            </div>

            <!-- Run orphans -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="flex items-center gap-1.5 pt-2 text-xs text-muted-foreground"><Unplug class="size-3.5" /> Orphans</Label>
              <div>
                <div class="flex h-8 items-center">
                  <Switch checked={graph.run_orphans ?? false} disabled={editLocked} onCheckedChange={(v) => toggleRunOrphans(graph, v)} />
                </div>
                <p class="text-[10px] leading-snug text-muted-foreground">
                  When off, nodes with no connections are skipped during a run.
                </p>
              </div>
            </div>

            <!-- Debug -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="flex items-center gap-1.5 pt-2 text-xs text-muted-foreground"><Bug class="size-3.5" /> Debug</Label>
              <div>
                <div class="flex h-8 items-center">
                  <Switch checked={graph.debug ?? false} disabled={editLocked} onCheckedChange={(v) => toggleDebug(graph, v)} />
                </div>
                <p class="text-[10px] leading-snug text-muted-foreground">
                  Log each node's result to the console during a run.
                </p>
              </div>
            </div>

            <!-- Trigger -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="pt-2 text-xs text-muted-foreground">Trigger</Label>
              <div class="space-y-1.5">
                <Select.Root
                  type="single"
                  disabled={editLocked}
                  value={triggerKind(graph.trigger)}
                  onValueChange={(v) => applyTrigger(graph, v ?? "manual", triggerSecs(graph.trigger))}
                >
                  <Select.Trigger class="h-8 w-full">{labelOf(TRIGGER_OPTS, triggerKind(graph.trigger))}</Select.Trigger>
                  <Select.Content>
                    {#each TRIGGER_OPTS as o (o.value)}
                      <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
                {#if triggerKind(graph.trigger) === "cron"}
                  <div class="flex items-center gap-2">
                    <Input type="number" min="1" disabled={editLocked} value={triggerSecs(graph.trigger)} class="h-8" onchange={(e) => applyTrigger(graph, "cron", Number(e.currentTarget.value))} />
                    <span class="text-xs text-muted-foreground">sec</span>
                  </div>
                {/if}
              </div>
            </div>

            <!-- Concurrency -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="pt-2 text-xs text-muted-foreground">Concurrency</Label>
              <div class="space-y-1.5">
                <Select.Root
                  type="single"
                  disabled={editLocked}
                  value={concKind(graph.concurrency)}
                  onValueChange={(v) => applyConcurrency(graph, v ?? "skip", concMax(graph.concurrency))}
                >
                  <Select.Trigger class="h-8 w-full">{labelOf(CONC_OPTS, concKind(graph.concurrency))}</Select.Trigger>
                  <Select.Content>
                    {#each CONC_OPTS as o (o.value)}
                      <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
                {#if concKind(graph.concurrency) !== "skip"}
                  <Input type="number" min="1" disabled={editLocked} value={concMax(graph.concurrency)} class="h-8" onchange={(e) => applyConcurrency(graph, concKind(graph.concurrency), Number(e.currentTarget.value))} />
                {/if}
              </div>
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
                <Button variant="ghost" size="icon" class="size-7 shrink-0" title="Delete node" disabled={editLocked} onclick={() => removeNode(selectedNode)}>
                  <Trash2 class="size-3.5" />
                </Button>
              </div>
              {#if configRows.length === 0}
                <p class="text-xs text-muted-foreground">This node has no settings.</p>
              {:else}
                <div class="space-y-2.5">
                  {#each configRows as { key, port } (key)}
                    {@const wired = wiredInto(selectedNode.id, key)}
                    <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
                      <Label class="flex items-center gap-1 pt-2 text-xs text-muted-foreground">
                        <span class="min-w-0 truncate">{key}</span>{#if port?.required}<span class="text-destructive">*</span>{/if}{#if wired}<Link2 class="size-3 shrink-0 text-primary" />{/if}
                      </Label>
                      <div class="min-w-0">
                      {#if wired}
                        <div class="rounded-lg border border-primary/30 bg-primary/5 px-2 py-1.5">
                          <div class="flex items-center justify-between gap-2">
                            <p class="flex items-center gap-1.5 text-xs font-medium text-primary">
                              <span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
                              from #{wired.from}.{wired.from_port}
                            </p>
                            <button
                              class="interactive inline-flex items-center gap-1 rounded-md border border-border bg-card px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground hover:text-destructive disabled:pointer-events-none disabled:opacity-50"
                              disabled={editLocked}
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
                          <Select.Root type="single" disabled={editLocked} value={cv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? cv })}>
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
                          <Select.Root type="single" disabled={editLocked} value={rv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? rv })}>
                            <Select.Trigger class="h-8 w-full">{rv}</Select.Trigger>
                            <Select.Content>
                              {#each recipientOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if port?.type === "dictionary"}
                        {#key selectedNode.id + ":" + key}
                          <DictionaryEditor
                            value={(selectedNode.config[key] as Record<string, unknown>) ?? {}}
                            disabled={editLocked}
                            onchange={(obj) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: obj })}
                          />
                        {/key}
                      {:else if port?.type === "file"}
                        {@const file = String(selectedNode.config[key] ?? "")}
                        {#if file}
                          <div class="flex items-center gap-2 rounded-lg border border-border bg-card px-2 py-1.5">
                            <FileIcon class="size-4 shrink-0 text-muted-foreground" />
                            <span class="min-w-0 flex-1 truncate text-xs" title={file}>{fileName(file)}</span>
                            <button
                              class="interactive rounded-md border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground disabled:pointer-events-none disabled:opacity-50"
                              disabled={editLocked}
                              onclick={() => pickFile(selectedNode, key)}
                            >
                              Change
                            </button>
                            <button
                              class="interactive rounded-md border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground hover:text-destructive disabled:pointer-events-none disabled:opacity-50"
                              disabled={editLocked}
                              onclick={() => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: "" })}
                            >
                              Clear
                            </button>
                          </div>
                        {:else}
                          <Button variant="outline" size="sm" class="h-8 w-full justify-start gap-2" disabled={editLocked} onclick={() => pickFile(selectedNode, key)}>
                            <Upload class="size-3.5" /> Upload file…
                          </Button>
                        {/if}
                      {:else if port?.type === "image"}
                        {@const img = String(selectedNode.config[key] ?? "")}
                        {#if img}
                          <div class="flex items-center gap-2 rounded-lg border border-border bg-card px-2 py-1.5">
                            <ImageIcon class="size-4 shrink-0 text-muted-foreground" />
                            <span class="min-w-0 flex-1 truncate text-xs" title={img}>{fileName(img)}</span>
                            <button
                              class="interactive rounded-md border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground disabled:pointer-events-none disabled:opacity-50"
                              disabled={editLocked}
                              onclick={() => pickImage(selectedNode, key)}
                            >
                              Change
                            </button>
                            <button
                              class="interactive rounded-md border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground hover:text-destructive disabled:pointer-events-none disabled:opacity-50"
                              disabled={editLocked}
                              onclick={() => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: "" })}
                            >
                              Clear
                            </button>
                          </div>
                        {:else}
                          <Button variant="outline" size="sm" class="h-8 w-full justify-start gap-2" disabled={editLocked} onclick={() => pickImage(selectedNode, key)}>
                            <Upload class="size-3.5" /> Upload image…
                          </Button>
                        {/if}
                      {:else if port?.type === "bool"}
                        <div class="flex h-8 items-center gap-2">
                          <Switch
                            checked={selectedNode.config[key] === true || selectedNode.config[key] === "true"}
                            disabled={editLocked}
                            onCheckedChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v })}
                          />
                          <span class="text-xs text-muted-foreground">
                            {selectedNode.config[key] === true || selectedNode.config[key] === "true" ? "True" : "False"}
                          </span>
                        </div>
                      {:else if key === "level"}
                        <Select.Root
                          type="single"
                          disabled={editLocked}
                          value={String(selectedNode.config[key] ?? "info")}
                          onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "info" })}
                        >
                          <Select.Trigger class="h-8 w-full capitalize">{String(selectedNode.config[key] ?? "info")}</Select.Trigger>
                          <Select.Content>
                            {#each LEVELS as l (l)}<Select.Item value={l} label={l} class="capitalize">{l}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else if key === "event"}
                        <!-- Event name persists like any other field (onchange → saveNodeConfig).
                             Registration into the tenant registry (so other graphs can listen +
                             it appears in the palette under its category) is an explicit step. -->
                        {@const evName = String(selectedNode.config[key] ?? "").trim()}
                        {@const known = events.find((e) => e.name === evName)}
                        <Input
                          class="h-8"
                          list="wf-events"
                          disabled={editLocked}
                          placeholder="EventName (e.g. Foo)"
                          value={String(selectedNode.config[key] ?? "")}
                          onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value.trim() })}
                        />
                        <datalist id="wf-events">
                          {#each events as e (e.name)}<option value={e.name}>{e.category}</option>{/each}
                        </datalist>
                        {#if evName && known}
                          <p class="mt-1 text-[10px] text-muted-foreground">Registered · category “{known.category}”</p>
                        {:else if evName}
                          <div class="mt-1.5 flex items-center gap-1.5">
                            <Input
                              class="h-8"
                              list="wf-event-cats"
                              disabled={editLocked}
                              placeholder="Category (default General)"
                              bind:value={newEventCategory}
                            />
                            <Button size="sm" class="h-8 shrink-0" disabled={editLocked} onclick={() => registerEvent(evName)}>
                              Register
                            </Button>
                          </div>
                          <datalist id="wf-event-cats">
                            {#each eventCategories as c (c.name)}<option value={c.name}></option>{/each}
                          </datalist>
                          <p class="mt-1 text-[10px] text-muted-foreground">
                            New event — Register it so other graphs can listen and it shows in the palette.
                          </p>
                        {/if}
                      {:else}
                        <Input class="h-8" disabled={editLocked} value={String(selectedNode.config[key] ?? "")} onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })} />
                      {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
              <!-- Memoize: cache the node's output for the run (default) vs recompute each
                   time it's reached. Turn OFF for loop bodies / live sources so loops iterate. -->
              <div class="flex items-center justify-between gap-2 border-t border-border pt-2.5">
                <Label
                  class="flex items-center gap-1.5 text-xs text-muted-foreground"
                  title="On: cache this node's output for the run and reuse it. Off: recompute every time the node runs — required for loop bodies and live sources (e.g. Get current frame)."
                >
                  Memoize output
                </Label>
                <Switch
                  checked={selectedNode.memoize !== false}
                  disabled={editLocked}
                  onCheckedChange={(v) => saveNodeMemoize(selectedNode, v)}
                />
              </div>
            </div>
          {:else}
            <div class="rounded-xl border border-dashed border-border p-4 text-center text-xs text-muted-foreground">
              Select a node to edit its properties.
            </div>
          {/if}
        </div>
        {/if}
      </div>

      {#if consoleOpen}
        <!-- splitter: drag to resize the console height -->
        <div
          class="group relative flex h-2 shrink-0 cursor-row-resize items-center justify-center"
          onpointerdown={startConsoleResize}
          onpointermove={onConsoleResizeMove}
          onpointerup={endConsoleResize}
          onpointercancel={endConsoleResize}
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize console"
        >
          <div
            class="h-1 w-10 rounded-full transition-colors {resizingConsole
              ? 'bg-primary'
              : 'bg-border group-hover:bg-primary/60'}"
          ></div>
        </div>
      {/if}

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
          <div class="overflow-y-auto border-t border-border p-4 font-mono text-sm" style="height: {consoleH}px">
            {#if logs.length === 0}
              <p class="text-muted-foreground">No runs yet — run the workflow to see node output here.</p>
            {:else}
              <div class="space-y-1">
                {#each logs as log, i (i)}
                  <div class="flex items-center gap-2">
                    <span class="shrink-0 text-muted-foreground">{log.time}</span>
                    {#if log.level}
                      <Printer class="size-3 shrink-0 {logLineClass(log)}" />
                    {:else if log.debug}
                      <Bug class="size-3 shrink-0 text-muted-foreground/70" />
                    {:else}
                      <StatusDot state={log.ok ? "online" : "danger"} />
                    {/if}
                    <span class="w-32 shrink-0 truncate {logLineClass(log)}">{log.name}</span>
                    {#if log.ms !== undefined}
                      <span class="w-16 shrink-0 text-right tabular-nums text-muted-foreground/70">{log.ms} ms</span>
                    {:else}
                      <span class="w-16 shrink-0"></span>
                    {/if}
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

<!-- ShowImage: the node's frame, enlarged -->
<Dialog.Root bind:open={imageModalOpen}>
  <Dialog.Content class="max-w-[90vw] sm:max-w-[80vw] lg:max-w-5xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <ImageIcon class="size-4 text-muted-foreground" />
        {imageModal?.label || "Show image"}
      </Dialog.Title>
    </Dialog.Header>
    {#if imageModal}
      <img
        src={imageModal.src}
        alt={imageModal.label || "frame"}
        class="max-h-[80vh] w-full rounded-lg border border-border object-contain"
      />
    {/if}
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
              {#each cat.kinds as k (k.label)}
                <button
                  class="interactive group flex w-full items-center justify-between gap-3 rounded-xl border border-border bg-card px-3 py-2 text-left"
                  onclick={() => addNodeKind(k.slug, k.presetEvent)}
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

<!-- Recent runs drawer — persisted run history for one graph (trigger, pod, status). -->
<Dialog.Root bind:open={runsOpen}>
  <Dialog.Content class="max-w-2xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <History class="size-4" /> Recent runs — {runsGraph?.name ?? ""}
      </Dialog.Title>
      <Dialog.Description>Manual, scheduled, and event-triggered runs, newest first.</Dialog.Description>
    </Dialog.Header>
    <div class="max-h-[60vh] space-y-1.5 overflow-y-auto">
      {#if runsList.length === 0}
        <p class="py-8 text-center text-sm text-muted-foreground">No runs recorded yet.</p>
      {:else}
        {#each runsList as r (r.id)}
          <div class="rounded-lg border border-border bg-card/40">
            <button
              class="interactive flex w-full items-center gap-2 px-3 py-2 text-left"
              onclick={() => toggleRunLogs(r.id)}
            >
              {#if runsExpanded === r.id}<ChevronDown class="size-3.5 shrink-0 text-muted-foreground" />{:else}<ChevronRight class="size-3.5 shrink-0 text-muted-foreground" />{/if}
              <span class="inline-flex w-16 shrink-0 items-center rounded-full border px-2 py-0.5 text-[10px] font-medium capitalize {runStatusClass(r.status)}">
                {r.status}
              </span>
              <span class="text-xs font-medium">{triggerSourceLabel(r.trigger_source)}</span>
              {#if r.trigger_source === "event" && r.event_name}
                <span class="truncate text-[11px] text-muted-foreground">
                  '{r.event_name}'{#if r.source_graph_name} ← {r.source_graph_name}{/if}
                </span>
              {/if}
              <span class="ml-auto shrink-0 text-[10px] text-muted-foreground">{r.started_at}</span>
              <span class="shrink-0 text-[10px] text-muted-foreground/70">· {r.pod_name}</span>
            </button>
            {#if runsExpanded === r.id}
              <div class="border-t border-border px-3 py-2">
                {#if r.result_summary}<p class="mb-1.5 text-[11px] text-muted-foreground">{r.result_summary}</p>{/if}
                {#if (runsLogs[r.id]?.length ?? 0) === 0}
                  <p class="text-[11px] text-muted-foreground/60">No detailed signals recorded.</p>
                {:else}
                  <div class="space-y-0.5 font-mono text-[10px] leading-relaxed">
                    {#each runsLogs[r.id] as line (line)}
                      <div class="flex gap-2">
                        <span class="shrink-0 uppercase text-muted-foreground/60">{line.kind}</span>
                        <span class="text-muted-foreground">{line.label ? `${line.label}: ` : ""}{line.message}</span>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>
