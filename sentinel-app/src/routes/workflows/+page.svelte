<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { data, onEvent } from "$lib/data";
  import { platformLogs, type PlatformLog } from "$lib/api";
  import { fmtTs } from "$lib/utils";
  import {
    CATEGORY_ORDER as PRESENCE_CATEGORIES,
    CATEGORY_STYLE as PRESENCE_CATEGORY_STYLE,
  } from "$lib/map/categories";
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
  import SearchableSelect from "$lib/components/searchable-select.svelte";

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
  import { portColor, typeColor } from "$lib/workflows/portColors";
  import type { PortType } from "$lib/workflows/portColors";

  import ContextSearch from "./components/ContextSearch.svelte";
  import VariablesPanel from "./components/VariablesPanel.svelte";

  // ---- types (the serialized core shapes) ----------------------------------
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
  type EventField = { name: string; type: PortType };
  type WorkflowEvent = { name: string; category: string; source: string; fields?: EventField[] };
  type EventCategory = { name: string; source: string };
  // The payload types an event field may carry (the wire-able data types; no exec/arrays in v1).
  const PAYLOAD_TYPES: { value: PortType; label: string }[] = [
    { value: "text", label: "Text" },
    { value: "num", label: "Number" },
    { value: "bool", label: "Bool" },
    { value: "image", label: "Image" },
    { value: "num_array", label: "Num array" },
    { value: "text_array", label: "Text array" },
    { value: "bbox", label: "Bounding box" },
    { value: "camera", label: "Camera" },
    { value: "recipient", label: "Recipient" },
    { value: "file", label: "File" },
    { value: "dictionary", label: "Dictionary" },
  ];
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
  type Variable = { name: string; type: PortType };
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
    variables?: Variable[];
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
  // Extra height inserted between the header divider and data ports on comparison/logic
  // nodes, used to render the inline expression (a OP b).
  const EXPR_H = 30;
  // ShowImage preview: a 16:9 thumbnail tucked below the node's ports.
  const PREVIEW_W = NODE_W - 16;
  function commentW(n: Node): number { return Math.max(120, Number(n.config.width) || 400); }
  function commentH(n: Node): number { return Math.max(60, Number(n.config.height) || 200); }
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

  // Context search: appears at wire-drop position, pre-filtered by the pending port type.
  let contextOpen = $state(false);
  let contextPos = $state({ x: 0, y: 0 });
  let contextPending = $state<{ node: number; port: string; portType: PortType } | null>(null);
  const contextPortType = $derived(contextPending?.portType ?? ("any" as PortType));
  // Edge via-points: purely visual bend handles stored in memory (key = "from:port:to:port").
  // The logical edge A→B is unchanged; the via dots just reposition the wire on screen.
  // Up to 2 via-points per edge; ordered by insertion (not by x).
  let edgeVias = $state<Record<string, { x: number; y: number }[]>>({});
  let dragVia = $state<{ key: string; idx: number } | null>(null);
  let hoveredEdgeKey = $state<string | null>(null);

  function edgeViaKey(e: { from: number; from_port: string; to: number; to_port: string }) {
    return `${e.from}:${e.from_port}:${e.to}:${e.to_port}`;
  }

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

  function platformLogToLine(l: PlatformLog): LogLine {
    const label = (l.payload?.label as string | undefined) ?? (l.payload?.graph_name as string | undefined);
    return {
      time: fmtTs(l.created_at),
      name: label ?? "workflow",
      ok: l.level !== "error",
      summary: l.message,
      debug: l.level === "debug",
    };
  }

  async function loadConsoleLogs(graphId: number) {
    try {
      const r = await platformLogs.list({
        module: "workflow",
        entity_type: "workflow_graph",
        entity_id: String(graphId),
        limit: 50,
      });
      logs = r.items.map(platformLogToLine);
    } catch { /* best-effort — console works without history */ }
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

  // TimeGate live countdown — keyed by node id, value is remaining seconds (null = idle).
  let timeGateCountdowns = $state<Record<number, number | null>>({});
  let timeGateTimer: ReturnType<typeof setInterval> | null = null;

  function startTimeGatePolling() {
    stopTimeGatePolling();
    timeGateTimer = setInterval(() => pollTimeGates(), 1000);
  }
  function stopTimeGatePolling() {
    if (timeGateTimer != null) { clearInterval(timeGateTimer); timeGateTimer = null; }
  }
  async function pollTimeGates() {
    const g = graph;
    if (!g) return;
    const nodes = g.nodes.filter((n) => n.kind === "time_gate");
    for (const n of nodes) {
      try {
        const url = data.nodeStateUrl(g.id, n.id);
        const r = await fetch(url);
        if (!r.ok) { timeGateCountdowns = { ...timeGateCountdowns, [n.id]: null }; continue; }
        const st = await r.json();
        if (!st || st.state !== "armed" || !st.expires_at) {
          timeGateCountdowns = { ...timeGateCountdowns, [n.id]: null };
        } else {
          const remaining = Math.ceil((new Date(st.expires_at).getTime() - Date.now()) / 1000);
          timeGateCountdowns = { ...timeGateCountdowns, [n.id]: Math.max(0, remaining) };
        }
      } catch { timeGateCountdowns = { ...timeGateCountdowns, [n.id]: null }; }
    }
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
  let devices = $state<{ id?: string; ip?: string; name: string; rtsp_url?: string | null }[]>([]);
  let recipients = $state<{ label: string; enabled: boolean }[]>([]);
  let rooms = $state<{ id: number; name: string }[]>([]);
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
  // Draft payload fields while registering a new event (name + wire type each).
  let newEventFields = $state<EventField[]>([]);

  // live cursor world position (for the `n`-to-add-here shortcut) + pending drop point
  let hoverWorld = $state<{ x: number; y: number } | null>(null);
  let hoverClient = $state<{ x: number; y: number }>({ x: 200, y: 60 }); // screen coords of cursor over canvas
  let dropAt: { x: number; y: number } | null = null;

  // recent-runs drawer (per graph in the list)
  let runsOpen = $state(false);
  let runsGraph = $state<Graph | null>(null);
  let runsList = $state<RunRecord[]>([]);
  let runsLogs = $state<Record<string, PlatformLog[]>>({});
  let runsExpanded = $state<string | null>(null);

  // Per-node highlight controller (driven by workflow-node events): each node
  // lights up immediately on arrival and stays lit for at least MIN_HIGHLIGHT_MS.
  // Multiple nodes can be live simultaneously (fast graphs / branches). The active
  // node keeps its highlight until the next event (or run end). Visual only.
  const MIN_HIGHLIGHT_MS = 700;
  // Use a plain record instead of Set — $state({}) property mutations are definitely
  // tracked by Svelte 5's reactive proxy (Set.add/delete tracking has been unreliable).
  const liveNodes = $state<Record<number, true>>({});
  const liveTimers = new Map<number, ReturnType<typeof setTimeout>>();
  let runEnded = false;

  function markLive(nodeId: number) {
    runEnded = false;
    const prev = liveTimers.get(nodeId);
    if (prev !== undefined) clearTimeout(prev); // re-trigger: restart the minimum hold
    liveNodes[nodeId] = true;
    liveTimers.set(nodeId, setTimeout(() => {
      liveTimers.delete(nodeId);
      // Keep the node lit if it's still the active node and the run hasn't ended.
      if (!runEnded && act?.current === nodeId) return;
      delete liveNodes[nodeId];
    }, MIN_HIGHLIGHT_MS));
  }
  function endLive() {
    runEnded = true;
    // Pending timers will fire and remove themselves. Nodes that outlasted their
    // timer (stayed lit as the active node) need to be cleared right now.
    for (const k of Object.keys(liveNodes)) {
      const id = Number(k);
      if (!liveTimers.has(id)) delete liveNodes[id];
    }
  }
  function clearLive() {
    for (const t of liveTimers.values()) clearTimeout(t);
    liveTimers.clear();
    for (const k in liveNodes) delete liveNodes[Number(k)];
    runEnded = false;
  }

  // ---- delete confirmation (window.confirm is a no-op in the Tauri WebView) --
  let deleteOpen = $state(false);
  let deleteTarget = $state<Graph | null>(null);

  const graph = $derived(graphs.find((g) => g.id === selectedGraphId) ?? null);
  const selectedNode = $derived(graph?.nodes.find((n) => n.id === selectedNodeId) ?? null);
  const nodeById = $derived(new Map(graph?.nodes.map((n) => [n.id, n]) ?? []));
  const nonCommentNodes = $derived(graph?.nodes.filter((n) => n.kind !== "comment") ?? []);
  // Set of "<toNodeId>:<toPort>" for every wired input — lets isFilled()/requiredUnfilled()
  // be O(1) instead of scanning graph.edges per port per node on every edge change.
  const filledPorts = $derived(new Set((graph?.edges ?? []).map((e) => `${e.to}:${e.to_port}`)));
  const kinds = $derived(Object.values(catalog));

  // Synthesize per-event palette entries (On/Emit event: <name>) from the tenant registry.
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

  // run state for the open graph
  const act = $derived(selectedGraphId != null ? activity[selectedGraphId] : undefined);
  const isRunning = $derived((act?.running ?? 0) > 0);
  // An enabled graph is "armed" (idle, waiting for its trigger) — and a running one
  // is executing. Either way it's ACTIVE, so editing is locked to avoid changing a
  // graph mid-flight; stop it (disable) to edit nodes, configs, or positions.
  const editLocked = $derived(isRunning || (graph?.enabled ?? false));
  // A node (and the edge feeding it) is "live" while it has an active highlight.
  function nodeLive(id: number): boolean {
    return id in liveNodes;
  }

  // Re-entering the editor mid-run (e.g. during a long Wait) won't get a fresh
  // workflow-node event until that node finishes — so adopt the polled "current"
  // node to resume the live animation where it left off.
  $effect(() => {
    const cur = act?.current ?? null;
    // `!runEnded` guards against the polled `current` lagging behind the run-end
    // event: without it we'd re-light the last node after the flow already ended.
    if (view === "editor" && cur !== null && !runEnded && Object.keys(liveNodes).length === 0) {
      markLive(cur);
    }
  });

  // option lists for camera / recipient node dropdowns (the real items)
  // Cameras bind by the STABLE device id (fall back to ip for legacy rows with no id), NOT
  // the name — so renaming a camera doesn't silently re-point the node. The label stays the
  // (current) name. `cameraValue` maps a stored ref (id | ip | name) to its canonical value.
  const cameraKey = (d: { id?: string; ip?: string }) => d.id || d.ip || "";
  const cameraOptions = $derived(devices.map((d) => ({ value: cameraKey(d), label: d.name })));
  const recipientOptions = $derived(recipients.map((r) => ({ value: r.label, label: r.label })));
  // Rooms for the `room` node dropdown (CreateAlert / RecordPresence). Stored BY NAME —
  // the flow node resolves the name to a room_id, and the map pulse locates by it. A
  // blank "(no room)" option keeps room optional (e.g. CreateAlert without a room).
  const NO_ROOM = "__none__"; // sentinel — bits-ui Select doesn't take an empty-string value
  const roomOptions = $derived([
    { value: NO_ROOM, label: "(no room)" },
    ...rooms.map((r) => ({ value: r.name, label: r.name })),
  ]);
  // Presence categories for the `category` dropdown (slug → friendly label).
  const categoryOptions = PRESENCE_CATEGORIES.map((c) => ({ value: c, label: PRESENCE_CATEGORY_STYLE[c].label }));
  function cameraValue(stored: string): string {
    const s = String(stored ?? "");
    const d = devices.find((x) => (x.id && x.id === s) || x.ip === s || x.name === s);
    return d ? cameraKey(d) : (cameraOptions[0]?.value ?? "");
  }
  // Resolve a stored value to a real option — falls back to the FIRST item in the
  // list (what "default to first" means) when the stored value is empty/missing.
  function resolveOpt(opts: { value: string; label: string }[], stored: string) {
    return opts.some((o) => o.value === stored) ? stored : (opts[0]?.value ?? "");
  }

  // When a CameraSwitch node is selected and has empty-string arms (the old default config
  // ["", ""] or un-configured arms), auto-save the normalized values so the engine can match.
  // cameraValue("") resolves to the first camera's ID — this fires once per selection and
  // doesn't re-run because saveNodeConfig updates the stored arms to non-empty strings.
  $effect(() => {
    const node = selectedNode;
    if (!node || node.kind !== "camera_switch" || editLocked || cameraOptions.length === 0) return;
    const arms = Array.isArray(node.config?.arms) ? (node.config.arms as string[]) : [];
    if (!arms.some((a) => !a)) return; // no empty arms — nothing to do
    const normalized = arms.map((a) => a || cameraValue(a));
    saveNodeConfig(node, { ...node.config, arms: normalized });
  });

  // A graph can only do something when it has at least one wired node — unless it
  // opts into running orphans. With no edges and orphans off, every node is
  // skipped, so a run is a no-op and Run is disabled.
  function canRun(g: Graph): boolean {
    return (g.run_orphans ?? false) || g.edges.length > 0;
  }

  // ---- required-input validation -------------------------------------------
  // A required input is "filled" when it's wired or has a usable config literal.
  function isFilled(node: Node, port: CatPort): boolean {
    if (filledPorts.has(`${node.id}:${port.name}`)) return true;
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
    const nk = effectiveKind(selectedNode);
    // Exec pins are pure wiring — never editable settings, so exclude them. Event payload
    // ports are managed by the Payload editor (and wired on the canvas), not as value rows.
    // StringSwitch arms are managed by the Arms editor below, not as a generic text row.
    const payloadNames = new Set(payloadFields(selectedNode).map((f) => f.name));
    const inputs = (nk?.inputs ?? []).filter((p) => p.type !== "exec" && !payloadNames.has(p.name));
    const rows: { key: string; port: CatPort | null }[] = inputs.map((p) => ({ key: p.name, port: p }));
    const MANAGED_KEYS: Record<string, string[]> = { string_switch: ["arms"], camera_switch: ["arms"], comment: ["width", "height"] };
    const managedForKind = new Set(MANAGED_KEYS[selectedNode.kind] ?? []);
    for (const key of Object.keys(nk?.default_config ?? {})) {
      if (!inputs.some((p) => p.name === key) && !managedForKind.has(key)) rows.push({ key, port: null });
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
    const k = effectiveKind(node);
    const list = side === "out" ? k?.outputs : k?.inputs;
    return list?.find((p) => p.name === port);
  }
  function isExec(node: Node, port: string, side: "in" | "out"): boolean {
    return portOf(node, port, side)?.type === "exec";
  }
  // Event payload ports: EmitEvent/OnEvent carry one port per field of their configured
  // event's schema, baked onto `config.payload` when the event is chosen. EmitEvent takes
  // them as inputs, OnEvent exposes them as outputs.
  function payloadFields(node: Node): EventField[] {
    if (node.kind !== "emit_event" && node.kind !== "on_event") return [];
    const p = node.config?.payload;
    return Array.isArray(p) ? (p as EventField[]).filter((f) => f && f.name) : [];
  }
  // The catalog kind for a node, augmented with its dynamic ports (if any), so the
  // existing render/wire/validate helpers see EmitEvent inputs / OnEvent outputs /
  // StringSwitch per-arm exec outputs.
  function effectiveKind(node: Node): CatKind | undefined {
    const k = catalog[node.kind];
    if (!k) return k;
    // StringSwitch: exec outputs are the user-defined arm labels + built-in "default".
    if (node.kind === "string_switch") {
      const arms: string[] = Array.isArray(node.config?.arms) ? (node.config.arms as string[]) : [];
      const armPorts: CatPort[] = [
        ...arms.map((a) => ({ name: String(a), type: "exec" as CatPort["type"], required: false })),
        { name: "default", type: "exec" as CatPort["type"], required: false },
      ];
      return { ...k, outputs: armPorts };
    }
    // CameraSwitch: exec outputs are numbered "arm_1", "arm_2", ... + built-in "default".
    if (node.kind === "camera_switch") {
      const arms: unknown[] = Array.isArray(node.config?.arms) ? (node.config.arms as unknown[]) : [];
      const armPorts: CatPort[] = [
        ...arms.map((_, i) => ({ name: `arm_${i + 1}`, type: "exec" as CatPort["type"], required: false })),
        { name: "default", type: "exec" as CatPort["type"], required: false },
      ];
      return { ...k, outputs: armPorts };
    }
    // Get/Set Variable: one `value` port typed by the variable (baked into config.type).
    if (node.kind === "get_variable" || node.kind === "set_variable") {
      const ty = (typeof node.config?.type === "string" ? node.config.type : "text") as CatPort["type"];
      const port: CatPort = { name: "value", type: ty, required: node.kind === "set_variable" };
      return node.kind === "get_variable" ? { ...k, outputs: [port] } : { ...k, inputs: [port] };
    }
    const fields = payloadFields(node);
    if (fields.length === 0) return k;
    const ports: CatPort[] = fields.map((f) => ({ name: f.name, type: f.type, required: false }));
    if (node.kind === "emit_event") return { ...k, inputs: [...k.inputs, ...ports] };
    if (node.kind === "on_event") return { ...k, outputs: [...k.outputs, ...ports] };
    return k;
  }

  // Node header title: the kind label, plus the bound event name for event nodes
  // (e.g. "Emit event: Foo") so it reads in the header instead of clashing with ports.
  function nodeTitle(n: Node, k: CatKind | undefined): string {
    const base = k?.label ?? n.kind;
    if (n.kind === "comment") return String(n.config.text ?? "Comment").trim() || "Comment";
    if (n.kind === "emit_event" || n.kind === "on_event") {
      const ev = String(n.config.event ?? "").trim();
      return ev ? `${base}: ${ev}` : base;
    }
    if (n.kind === "object_detection") {
      const cls = String(n.config.class ?? "person").trim();
      return cls ? `${base}: ${cls}` : base;
    }
    if (n.kind === "string_switch" || n.kind === "camera_switch") {
      const arms: unknown[] = Array.isArray(n.config?.arms) ? (n.config.arms as unknown[]) : [];
      return arms.length > 0 ? `${base} (${arms.length})` : base;
    }
    if (n.kind === "get_variable" || n.kind === "set_variable") {
      const v = String(n.config.variable ?? "").trim();
      return v ? `${base}: ${v}` : base;
    }
    return base;
  }

  // Data ports = everything except the white exec pins.
  function dataPorts(k: CatKind | undefined, side: "in" | "out"): CatPort[] {
    const list = side === "in" ? k?.inputs : k?.outputs;
    return (list ?? []).filter((p) => p.type !== "exec");
  }
  function execOutPorts(k: CatKind | undefined): CatPort[] {
    return (k?.outputs ?? []).filter((p) => p.type === "exec");
  }
  // True when a node has multiple exec outputs (arms/branches). These nodes place all exec
  // pins in the body instead of the header, and skip the standard single exec_out header pin.
  function hasMultipleExecOuts(k: CatKind | undefined): boolean {
    return execOutPorts(k).length > 1;
  }

  // Friendly label for an exec output port — for camera_switch arms shows the camera name
  // instead of the internal "arm_1" key.
  function execPortLabel(node: Node, portName: string): string {
    if (portName === "default") return "default";
    if (node.kind === "camera_switch" && portName.startsWith("arm_")) {
      const idx = parseInt(portName.slice(4)) - 1;
      const arms = Array.isArray(node.config?.arms) ? (node.config.arms as string[]) : [];
      const stored = String(arms[idx] ?? "");
      return cameraOptions.find((o) => o.value === cameraValue(stored))?.label ?? portName;
    }
    return portName;
  }

  // Output data ports stack BELOW exec-out pins that live in the body.
  // Multi-exec nodes: ALL exec rows are in the body → offset = execCount.
  // Single-exec node: exec pin is in the header → no body rows consumed → offset = 0.
  function outDataOffset(k: CatKind | undefined): number {
    const count = execOutPorts(k).length;
    return count > 1 ? count : 0;
  }
  // Y where a node's body ends and the optional ShowImage preview begins.
  function portsBottom(n: Node): number {
    const k = effectiveKind(n);
    const execCount = execOutPorts(k).length;
    const outRows = dataPorts(k, "out").length + outDataOffset(k);
    const rows = Math.max(dataPorts(k, "in").length, outRows, execCount, 1);
    return HEAD_H + nodeExprH(n) + rows * ROW_H;
  }
  // A ShowImage node grows to fit its preview thumbnail once a frame has arrived.
  function hasPreview(n: Node): boolean {
    return n.kind === "show_image" && !!nodeImages[n.id];
  }
  function nodeHeight(n: Node): number {
    const extra = hasPreview(n) ? PREVIEW_H + PAD : n.kind === "time_gate" ? 26 : 0;
    return portsBottom(n) + PAD + extra;
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
      const k = effectiveKind(node);
      const i = execOutPorts(k).findIndex((p) => p.name === port);
      // Multi-exec: all exec pins live in the body; single-exec: header position.
      if (hasMultipleExecOuts(k)) return HEAD_H + Math.max(0, i) * ROW_H + ROW_H / 2;
      return HEAD_H / 2;
    }
    const k = effectiveKind(node);
    const i = dataPorts(k, side).findIndex((p) => p.name === port);
    const off = side === "out" ? outDataOffset(k) : 0;
    return dataPortY(Math.max(0, i) + off) + nodeExprH(node);
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
  // Pure data/logic nodes: no exec pins, activated by data availability.
  // Includes comparison operators, logical operators, and type-conversion nodes.
  const PURE_KINDS = new Set([
    "num_lt","num_lte","num_gt","num_gte","num_eq","num_ne","compare","and","or","not","text_compare",
    "bool_to_text","num_to_text","get_camera_by_name","break_camera","get_variable",
  ]);
  function isCompareOrLogic(kind: string): boolean {
    return PURE_KINDS.has(kind);
  }
  function isPure(k: CatKind | undefined): boolean {
    return !((k?.inputs ?? []).some((p) => p.type === "exec"));
  }
  function nodeExprH(n: Node): number {
    return isCompareOrLogic(n.kind) ? EXPR_H : 0;
  }
  function opLabel(kind: string, config: Record<string, unknown>): string {
    switch (kind) {
      case "num_lt":  return "<";
      case "num_lte": return "≤";
      case "num_gt":  return ">";
      case "num_gte": return "≥";
      case "num_eq":  return "=";
      case "num_ne":  return "≠";
      case "compare": return String(config.operator ?? "==");
      case "and":          return "AND";
      case "or":           return "OR";
      case "not":          return "NOT";
      case "text_compare": return String(config.operator ?? "eq");
      default:             return "?";
    }
  }
  function exprOperand(nodeId: number, port: string, config: Record<string, unknown>): string {
    const edge = wiredInto(nodeId, port);
    if (edge) {
      const src = graph?.nodes.find((n) => n.id === edge.from);
      const srcLabel = src ? (catalog[src.kind]?.label ?? src.kind) : "?";
      const lbl = `${srcLabel}.${edge.from_port}`;
      return lbl.length > 15 ? lbl.slice(0, 14) + "…" : lbl;
    }
    return String(config[port] ?? "");
  }
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
    // Pre-fetch persisted ShowImage frames (DB-backed, survives flow restarts).
    const showNodes = graphs.find((x) => x.id === id)?.nodes.filter((n: Node) => n.kind === "show_image") ?? [];
    for (const n of showNodes) {
      const url = data.runFrameUrl(id, n.id);
      fetch(url).then((r) => {
        if (r.ok) {
          const label = typeof n.config?.label === "string" ? n.config.label : "";
          nodeImages = { ...nodeImages, [n.id]: { src: `${url}&t=${Date.now()}`, label } };
        }
      }).catch(() => {});
    }
    logs = [];
    void loadConsoleLogs(id);
    const v = graphs.find((x) => x.id === id)?.viewport;
    pan = v ? { x: v.x, y: v.y } : { x: 40, y: 40 };
    zoom = v?.zoom ?? 1;
    clearLive();
    view = "editor";
    void reconcileEventNodes(id); // self-heal event nodes against the current registry
    startTimeGatePolling();
  }
  function backToList() {
    stopTimeGatePolling();
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
      const [d, r, m] = await Promise.all([data.listDevices(), data.listBeacons(), data.loadMap()]);
      devices = d;
      recipients = r;
      rooms = (m.rooms ?? []).map((rm) => ({ id: rm.id, name: rm.name }));
    } catch (err) {
      console.error("Failed to load devices/recipients/rooms", err);
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

  // Seed the live activity map from REST. Called once on mount and again on every SSE
  // (re)connect — between connects we'd miss `workflow-activity` deltas, so re-fetch.
  async function refreshActivity() {
    try {
      activity = await data.runStates<Record<number, Activity>>();
    } catch {
      /* ignore transient errors */
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
        await refreshActivity();
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
      // Run finished (ok or error) → drain remaining highlights, then stop. The console
      // shows only the open graph; other graphs' completions still surface as toasts.
      if (gid === selectedGraphId) {
        endLive();
        logs = [{ time: fmtTs(new Date().toISOString()), name, ok, summary }, ...logs].slice(0, 200);
      }
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
        { time: fmtTs(new Date().toISOString()), name: label, ok: true, summary: message, debug: true, ms },
        ...logs,
      ].slice(0, 200);
    });
    // Print node: echo to the Console (independent of the debug flag) for the OPEN graph
    // only, and when on_screen flash it over the canvas.
    const unPrint = onEvent("workflow-print", (payload) => {
      const p = payload as { graph?: number; message: string; level: string; on_screen: boolean };
      if (p.graph !== selectedGraphId) return; // only the open graph
      logs = [
        { time: fmtTs(new Date().toISOString()), name: "Print", ok: true, summary: p.message, level: p.level },
        ...logs,
      ].slice(0, 200);
      if (p.on_screen) flashOnScreen(p.message, p.level);
    });
    // Show image node: the event is a reference (bytes never ride SSE) — fetch the frame
    // out of band from the control relay, cache-busting so each new frame reloads.
    const unImage = onEvent("workflow-image", (payload) => {
      const { graph: gid, node, label } = payload as {
        graph: number;
        node: number;
        label: string;
      };
      if (gid !== selectedGraphId) return; // node ids are per-graph — only the open one
      nodeImages = { ...nodeImages, [node]: { src: data.runFrameUrl(gid, node, Date.now()), label } };
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
        { time: fmtTs(new Date().toISOString()), name: "Event", ok: p.kind !== "skip", summary, level: p.kind === "skip" ? "warn" : "info" },
        ...logs,
      ].slice(0, 200);
    });
    // Live run activity (running graphs + current node) for the lock + list badges —
    // pushed from the engine over SSE (replaces the old 700 ms poll). Each delta is one
    // graph's snapshot; merge it into the map.
    const unActivity = onEvent("workflow-activity", (payload) => {
      const p = payload as {
        graph: number;
        running: number;
        current: number | null;
        run_id?: string | null;
        trigger?: string | null;
        pod_name?: string | null;
      };
      activity = {
        ...activity,
        [p.graph]: {
          running: p.running,
          current: p.current,
          run_id: p.run_id,
          trigger: p.trigger,
          pod_name: p.pod_name,
        },
      };
      // Self-heal the run animation from authoritative state: when the engine reports the
      // open graph is no longer running, drain any highlights even if the separate
      // `workflow-run` event was missed — otherwise a node can stay lit indefinitely.
      if (p.graph === selectedGraphId && p.running === 0) endLive();
    });
    // Re-seed from REST on every (re)connect — deltas emitted while disconnected are missed.
    const unOpen = onEvent("stream-open", () => {
      void refreshActivity();
    });
    return () => {
      saveViewport(); // persist the viewport when leaving the page too
      un.then((u) => u());
      unNode.then((u) => u());
      unLog.then((u) => u());
      unPrint.then((u) => u());
      unImage.then((u) => u());
      unEvent.then((u) => u());
      unActivity.then((u) => u());
      unOpen.then((u) => u());
      screenTimers.forEach((t) => clearTimeout(t));
      screenTimers.clear();
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
  async function addNodeKind(slug: string, presetEvent?: string): Promise<number | null> {
    if (!graph || editLocked) return null;
    // Place at the cursor (when opened via the `n` shortcut), else cascade.
    let x: number;
    let y: number;
    if (dropAt) {
      x = Math.round(dropAt.x) - NODE_W / 2;
      y = Math.round(dropAt.y) - HEAD_H / 2;
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
      cfg.camera = cameraKey(devices[0]); // bind by stable id (not name)
      patched = true;
    }
    if ("recipient" in cfg && recipients[0]) {
      cfg.recipient = recipients[0].label;
      patched = true;
    }
    if (presetEvent) {
      cfg.event = presetEvent;
      cfg.payload = eventFields(presetEvent); // bake payload ports onto the new event node
      patched = true;
    }
    if (patched) await data.setNodeConfig(graph.id, id, cfg);
    selectedNodeId = id;
    await reload();
    return id;
  }

  // ---- workflow variables (left panel) -------------------------------------
  let varsOpen = $state(true);
  const variables = $derived<Variable[]>(graph?.variables ?? []);

  async function saveVariables(next: Variable[]) {
    if (!graph) return;
    await data.setGraphVariables(graph.id, next.map((v) => ({ name: v.name, type: v.type })));
    await reload();
  }
  function uniqueVarName(existing: Variable[]): string {
    const taken = new Set(existing.map((v) => v.name));
    let i = existing.length + 1;
    while (taken.has(`var${i}`)) i++;
    return `var${i}`;
  }
  async function addVariable() {
    if (editLocked) return;
    await saveVariables([...variables, { name: uniqueVarName(variables), type: "text" as PortType }]);
  }
  // Edit a variable + re-sync the baked name/type on any Get/Set nodes that reference it,
  // so existing nodes keep a valid port type after a rename/retype.
  async function editVariable(i: number, patch: { name?: string; type?: string }) {
    if (editLocked || !graph) return;
    const old = variables[i];
    if (!old) return;
    const updated = { ...old, ...patch } as Variable;
    if (patch.name === "" || (patch.name && variables.some((v, j) => j !== i && v.name === patch.name))) return; // keep names non-empty + unique
    for (const n of graph.nodes) {
      if ((n.kind === "get_variable" || n.kind === "set_variable") && n.config?.variable === old.name) {
        await data.setNodeConfig(graph.id, n.id, { variable: updated.name, type: updated.type });
      }
    }
    await saveVariables(variables.map((v, j) => (j === i ? updated : v)));
  }
  async function removeVariable(i: number) {
    if (editLocked) return;
    await saveVariables(variables.filter((_, j) => j !== i));
  }
  // Drop a Get/Set node for a variable onto the canvas, with its name + type baked into config.
  async function addVariableNode(kind: "get_variable" | "set_variable", v: { name: string; type: string }) {
    if (!graph || editLocked) return;
    const id = await addNodeKind(kind);
    if (id === null) return;
    await data.setNodeConfig(graph.id, id, { variable: v.name, type: v.type });
    await reload();
  }

  // Add a node from context search and auto-wire it to the pending output port.
  async function addFromContext(slug: string, presetEvent?: string) {
    const cp = contextPending;
    contextOpen = false;
    contextPending = null;
    const id = await addNodeKind(slug, presetEvent);
    if (cp && id !== null && graph) {
      const cat = catalog[slug];
      const matchPort =
        cat?.inputs.find((p) => p.type === cp.portType) ??
        cat?.inputs.find((p) => p.type === "any");
      if (matchPort) {
        try {
          await data.addEdge(graph.id, cp.node, cp.port, id, matchPort.name);
          await reload();
        } catch {}
      }
    }
  }

  // Keyboard shortcuts over the canvas (ignored while typing in a field or graph is active).
  function onWindowKey(e: KeyboardEvent) {
    if (view !== "editor") return;
    const el = document.activeElement as HTMLElement | null;
    const tag = el?.tagName;
    const inField = tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el?.isContentEditable;

    // Escape: cancel pending wire OR deselect node (priority order).
    if (e.key === "Escape") {
      if (pending) { pending = null; e.preventDefault(); return; }
      if (selectedNodeId !== null) { selectedNodeId = null; e.preventDefault(); return; }
      return;
    }

    if (inField) return;  // remaining shortcuts require no active text input

    // n / N: open node search palette at cursor position.
    if ((e.key === "n" || e.key === "N") && !editLocked && !contextOpen) {
      e.preventDefault();
      dropAt = hoverWorld;
      contextPos = hoverClient;
      contextPending = null;
      contextOpen = true;
      return;
    }

    // Delete / Backspace: delete the selected node.
    if ((e.key === "Delete" || e.key === "Backspace") && !editLocked) {
      const sel = graph?.nodes.find((n) => n.id === selectedNodeId);
      if (sel) { e.preventDefault(); removeNode(sel); }
      return;
    }
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
      case "Cameras": return portColor.camera; // sky
      case "Recipients": return portColor.recipient; // purple
      case "Text": return portColor.num; // green
      case "Network": return "#22d3ee"; // cyan
      default: return "#64748b";
    }
  }

  // Old field names whose (name, type) isn't preserved in the other schema — their wires
  // are invalid and get pruned so the ports can be re-wired.
  function staleFields(oldFields: EventField[], newFields: EventField[]): string[] {
    return oldFields.filter((o) => !newFields.some((n) => n.name === o.name && n.type === o.type)).map((f) => f.name);
  }
  const draftFields = (): EventField[] =>
    newEventFields.map((f) => ({ name: f.name.trim(), type: f.type })).filter((f) => f.name);

  // Save the event's schema to the registry + bake it onto the open node. Other graphs that
  // use this event self-heal when next opened (see `reconcileEventNodes`) — no manual step.
  async function registerEvent(name: string) {
    const ev = name.trim();
    if (!ev) return;
    const fields = draftFields();
    try {
      await data.createEvent(ev, newEventCategory.trim() || "General", undefined, fields);
      await loadEvents();
      if (selectedNode && String(selectedNode.config.event ?? "").trim() === ev) {
        await saveNodeConfig(selectedNode, { ...selectedNode.config, event: ev, payload: fields });
      }
      payloadEditorFor = null; // let the effect re-seed the editor from the saved schema
      toast.success(`Event “${ev}” saved`);
    } catch (err) {
      toast.error("Couldn't save event", { description: String(err) });
    }
  }

  // On opening a graph, bring its Emit/On nodes in line with the current event registry:
  // re-bake each node's payload schema and drop wires on any port that was removed/retyped.
  // Silent when nothing changed; otherwise alerts how many nodes were updated.
  async function reconcileEventNodes(graphId: number) {
    const g = graphs.find((x) => x.id === graphId);
    if (!g) return;
    const removeIdx: number[] = [];
    const updates: { id: number; config: Record<string, unknown> }[] = [];
    for (const n of g.nodes) {
      if (n.kind !== "emit_event" && n.kind !== "on_event") continue;
      const ev = String(n.config.event ?? "").trim();
      const known = events.find((e) => e.name === ev);
      if (!ev || !known) continue;
      const want = known.fields ?? [];
      const have = (Array.isArray(n.config.payload) ? n.config.payload : []) as EventField[];
      if (staleFields(have, want).length === 0 && staleFields(want, have).length === 0) continue; // in sync
      const stale = new Set(staleFields(have, want));
      g.edges.forEach((e, idx) => {
        const emitHit = n.kind === "emit_event" && e.to === n.id && stale.has(e.to_port);
        const onHit = n.kind === "on_event" && e.from === n.id && stale.has(e.from_port);
        if (emitHit || onHit) removeIdx.push(idx);
      });
      updates.push({ id: n.id, config: { ...n.config, payload: want } });
    }
    if (updates.length === 0) return;
    try {
      for (const idx of removeIdx.sort((a, b) => b - a)) await data.removeEdge(graphId, idx);
      for (const u of updates) await data.setNodeConfig(graphId, u.id, u.config);
      await reload();
      toast.info(`Updated ${updates.length} event node(s) to the latest payload`);
    } catch (err) {
      console.error("reconcile event nodes failed", err);
    }
  }

  // Payload schema of a registered event (empty if unknown / payload-less).
  function eventFields(name: string): EventField[] {
    return events.find((e) => e.name === name)?.fields ?? [];
  }
  // Choose an event for an EmitEvent/OnEvent node: persist the name AND bake the event's
  // payload schema onto the node so its dynamic ports (one per field) render + wire.
  function chooseEvent(node: Node, raw: string) {
    const ev = raw.trim();
    saveNodeConfig(node, { ...node.config, event: ev, payload: eventFields(ev) });
  }
  function addEventField() {
    newEventFields = [...newEventFields, { name: "", type: "text" }];
  }
  function removeEventField(i: number) {
    newEventFields = newEventFields.filter((_, idx) => idx !== i);
  }
  function setEventFieldName(i: number, name: string) {
    newEventFields = newEventFields.map((f, idx) => (idx === i ? { ...f, name } : f));
  }
  function setEventFieldType(i: number, type: PortType) {
    newEventFields = newEventFields.map((f, idx) => (idx === i ? { ...f, type } : f));
  }
  function payloadTypeLabel(t: PortType): string {
    return PAYLOAD_TYPES.find((x) => x.value === t)?.label ?? t;
  }
  // Seed the payload-fields editor whenever the selected event node points at a different
  // event: load that event's saved fields (empty for a brand-new name) so the editor always
  // reflects the current schema and can edit it — not just define a new one.
  let payloadEditorFor = $state<string | null>(null);
  $effect(() => {
    const ev =
      selectedNode && (selectedNode.kind === "emit_event" || selectedNode.kind === "on_event")
        ? String(selectedNode.config.event ?? "").trim()
        : "";
    if (ev !== payloadEditorFor) {
      payloadEditorFor = ev;
      const known = events.find((e) => e.name === ev);
      newEventFields = known?.fields?.map((f) => ({ ...f })) ?? [];
      newEventCategory = known?.category ?? "";
    }
  });

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
        const r = await platformLogs.list({ trace_id: runId, module: "workflow", limit: 100 });
        runsLogs = { ...runsLogs, [runId]: r.items };
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
    const removed = graph.edges[idx];
    if (!removed) return;
    const gid = graph.id;
    // Optimistic: drop the wire locally now, persist in the background. No full
    // reload — the canvas updates instantly and we revert only if the server says no.
    graph.edges.splice(idx, 1);
    const vk = edgeViaKey(removed);
    if (edgeVias[vk]) delete edgeVias[vk];
    try {
      await data.removeEdge(gid, idx);
    } catch (err) {
      graph.edges.splice(idx, 0, removed);
      toast.error("Couldn't remove wire", { description: String(err) });
    }
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
    const gid = graph.id;
    const newEdge: Edge = { from: pending.node, from_port: pending.port, to: node.id, to_port: port };
    pending = null;
    // Optimistic: show the wire immediately, persist in the background. Revert if
    // the server rejects it (e.g. type mismatch / cycle).
    graph.edges.push(newEdge);
    try {
      await data.addEdge(gid, newEdge.from, newEdge.from_port, newEdge.to, newEdge.to_port);
    } catch (err) {
      const i = graph.edges.indexOf(newEdge);
      if (i >= 0) graph.edges.splice(i, 1);
      toast.error("Can't wire that", { description: String(err) });
    }
  }
  async function saveNodeConfig(n: Node, config: Record<string, unknown>) {
    if (!graph) return;
    await data.setNodeConfig(graph.id, n.id, config);
    await reload();
  }

  // Live-save a single config field as the user types: update locally at once (so the
  // value sticks + downstream reads it without pressing Enter) and debounce the backend
  // write (config-only, no reload — that would fight the input mid-type).
  let cfgSaveTimer: ReturnType<typeof setTimeout> | undefined;
  function liveSaveConfig(n: Node, key: string, value: unknown) {
    if (!graph) return;
    const gid = graph.id;
    n.config = { ...n.config, [key]: value };
    const cfg = n.config;
    clearTimeout(cfgSaveTimer);
    cfgSaveTimer = setTimeout(() => {
      data.setNodeConfig(gid, n.id, cfg).catch((err) => console.error("Failed to save node config", err));
    }, 400);
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
  let drag: { id: number; offX: number; offY: number; enclosedIds?: number[] } | null = null;
  let commentResize: { id: number; startX: number; startY: number; startW: number; startH: number } | null = null;
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
    const base = { id: n.id, offX: wp.x - n.x, offY: wp.y - n.y };
    if (n.kind === 'comment' && graph) {
      const cw = commentW(n), ch = commentH(n);
      const enclosedIds = graph.nodes
        .filter(r => r.kind !== 'comment'
          && r.x + NODE_W / 2 >= n.x && r.x + NODE_W / 2 <= n.x + cw
          && r.y + nodeHeight(r) / 2 >= n.y && r.y + nodeHeight(r) / 2 <= n.y + ch)
        .map(r => r.id);
      drag = { ...base, enclosedIds };
    } else {
      drag = base;
    }
  }
  function startCommentResize(e: PointerEvent, n: Node) {
    e.stopPropagation();
    if (editLocked) return;
    commentResize = { id: n.id, startX: e.clientX, startY: e.clientY, startW: commentW(n), startH: commentH(n) };
    svgEl?.setPointerCapture?.(e.pointerId);
  }
  function onCanvasDown(e: PointerEvent) {
    if (e.target !== e.currentTarget) return; // only grab empty canvas, not a node
    if (pending) {
      // Dropping a wire onto empty canvas → open context-aware node search at the cursor.
      if (svgEl) {
        const pn = nodeById.get(pending.node);
        const portType = (pn ? effectiveKind(pn)?.outputs.find((p) => p.name === pending.port)?.type : null) ?? "any";
        dropAt = hoverWorld;
        contextPending = { node: pending.node, port: pending.port, portType: portType as PortType };
        contextPos = { x: e.clientX, y: e.clientY };
        contextOpen = true;
      }
      pending = null;
      return;
    }
    selectedNodeId = null; // clicking the empty canvas deselects the node
    panning = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
    svgEl?.setPointerCapture?.(e.pointerId);
  }
  let _canvasPendingMove: PointerEvent | null = null;
  let _canvasMoveRaf: number | null = null;

  function onCanvasMove(e: PointerEvent) {
    _canvasPendingMove = e;
    if (_canvasMoveRaf !== null) return;
    _canvasMoveRaf = requestAnimationFrame(() => {
      _canvasMoveRaf = null;
      const ev = _canvasPendingMove;
      if (ev) { _canvasPendingMove = null; _onCanvasMove(ev); }
    });
  }

  function _onCanvasMove(e: PointerEvent) {
    hoverWorld = worldPoint(e); // track the cursor for the `n`-to-add-here shortcut
    hoverClient = { x: e.clientX, y: e.clientY };
    if (pending) cursor = worldPoint(e); // drag the open wire end with the cursor
    if (dragVia) {
      const wp = worldPoint(e);
      const cur = edgeVias[dragVia.key] ? [...edgeVias[dragVia.key]] : [];
      cur[dragVia.idx] = wp;
      edgeVias[dragVia.key] = cur;
      return;
    }
    if (panning) {
      pan = { x: panning.ox + (e.clientX - panning.sx), y: panning.oy + (e.clientY - panning.sy) };
      return;
    }
    if (commentResize && graph) {
      const dx = (e.clientX - commentResize.startX) / zoom;
      const dy = (e.clientY - commentResize.startY) / zoom;
      const n = nodeById.get(commentResize.id);
      if (n) n.config = { ...n.config, width: Math.max(120, Math.round(commentResize.startW + dx)), height: Math.max(60, Math.round(commentResize.startH + dy)) };
    } else if (drag && graph) {
      const wp = worldPoint(e);
      const n = nodeById.get(drag.id);
      if (n) {
        const newX = Math.round(wp.x - drag.offX);
        const newY = Math.round(wp.y - drag.offY);
        const dx = newX - n.x, dy = newY - n.y;
        n.x = newX; n.y = newY;
        if (drag.enclosedIds?.length) {
          for (const eid of drag.enclosedIds) {
            const en = nodeById.get(eid);
            if (en) { en.x += dx; en.y += dy; }
          }
        }
      }
    }
  }
  async function onCanvasUp() {
    if (dragVia) {
      dragVia = null;
      return;
    }
    if (panning) {
      panning = null;
      return;
    }
    if (commentResize && graph) {
      const id = commentResize.id;
      const n = graph.nodes.find((x) => x.id === id);
      commentResize = null;
      if (n) await saveNodeConfig(n, n.config);
      return;
    }
    if (drag && graph) {
      const id = drag.id;
      const enclosedIds = drag.enclosedIds ?? [];
      const n = graph.nodes.find((x) => x.id === id);
      drag = null;
      if (n) await data.moveNode(graph.id, id, n.x, n.y);
      for (const eid of enclosedIds) {
        const en = graph.nodes.find(x => x.id === eid);
        if (en) await data.moveNode(graph.id, eid, en.x, en.y);
      }
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
    const maxX = Math.max(...graph.nodes.map((n) => n.x + (n.kind === 'comment' ? commentW(n) : NODE_W))) + 60;
    const maxY = Math.max(...graph.nodes.map((n) => n.y + (n.kind === 'comment' ? commentH(n) : nodeHeight(n)))) + 60;
    const r = svgEl.getBoundingClientRect();
    const z = clampZoom(Math.min(r.width / (maxX - minX), r.height / (maxY - minY), 1.5));
    zoom = z;
    pan = {
      x: (r.width - (maxX - minX) * z) / 2 - minX * z,
      y: (r.height - (maxY - minY) * z) / 2 - minY * z,
    };
  }

  const LEVELS = ["debug", "info", "warn", "error"];
  const COCO_CLASSES = [
    "person","bicycle","car","motorcycle","airplane","bus","train","truck","boat",
    "traffic light","fire hydrant","stop sign","parking meter","bench","bird","cat",
    "dog","horse","sheep","cow","elephant","bear","zebra","giraffe","backpack",
    "umbrella","handbag","tie","suitcase","frisbee","skis","snowboard","sports ball",
    "kite","baseball bat","baseball glove","skateboard","surfboard","tennis racket",
    "bottle","wine glass","cup","fork","knife","spoon","bowl","banana","apple",
    "sandwich","orange","broccoli","carrot","hot dog","pizza","donut","cake",
    "chair","couch","potted plant","bed","dining table","toilet","tv","laptop",
    "mouse","remote","keyboard","cell phone","microwave","oven","toaster","sink",
    "refrigerator","book","clock","vase","scissors","teddy bear","hair drier","toothbrush",
  ];
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

      <!-- row 1: variables panel + editor + resizable side panel -->
      <div class="flex min-h-0 flex-1 gap-1">
        <!-- col 0: variables (Unreal-style) -->
        <VariablesPanel
          bind:open={varsOpen}
          {variables}
          types={PAYLOAD_TYPES}
          disabled={editLocked}
          onAdd={addVariable}
          onEdit={editVariable}
          onRemove={removeVariable}
          onAddNode={addVariableNode}
        />
        <!-- col 1: the node editor -->
        <div class="relative min-w-0 flex-1">
          <div
            class="absolute inset-0 overflow-hidden rounded-xl border border-border bg-well"
            style="background-image: linear-gradient(oklch(1 0 0 / 9%) 1px, transparent 1px), linear-gradient(90deg, oklch(1 0 0 / 9%) 1px, transparent 1px), linear-gradient(oklch(1 0 0 / 4%) 1px, transparent 1px), linear-gradient(90deg, oklch(1 0 0 / 4%) 1px, transparent 1px); background-size: {90 * zoom}px {90 * zoom}px, {90 * zoom}px {90 * zoom}px, {18 * zoom}px {18 * zoom}px, {18 * zoom}px {18 * zoom}px; background-position: {pan.x}px {pan.y}px, {pan.x}px {pan.y}px, {pan.x}px {pan.y}px, {pan.x}px {pan.y}px;"
          >
            <svg
              bind:this={svgEl}
              use:passiveWheel
              data-canvas
              class="block size-full {panning ? 'cursor-grabbing' : 'cursor-grab'}"
              onpointerdown={onCanvasDown}
              onpointermove={onCanvasMove}
              onpointerup={onCanvasUp}
              onpointerleave={onCanvasUp}
              role="presentation"
            >
              <defs>
                <filter id="nodeGlow" x="-40%" y="-40%" width="180%" height="180%">
                  <feDropShadow dx="0" dy="0" stdDeviation="5" flood-color="#e5e5e5" flood-opacity="0.5" />
                </filter>
              </defs>

              <g transform="translate({pan.x} {pan.y}) scale({zoom})">
              <!-- comment boxes (behind edges and regular nodes) -->
              {#each graph.nodes.filter(cn => cn.kind === 'comment') as n (n.id)}
                {@const cw = commentW(n)}
                {@const ch = commentH(n)}
                {@const col = String(n.config.color ?? '#facc15')}
                {@const sel = selectedNodeId === n.id}
                <g transform="translate({n.x} {n.y})">
                  <rect
                    width={cw} height={ch} rx="10"
                    fill={col} fill-opacity="0.1"
                    stroke={col} stroke-opacity={sel ? 0.7 : 0.35} stroke-width={sel ? 2 : 1.5}
                    class="cursor-move"
                    onpointerdown={(ev) => startDrag(ev, n)}
                    role="presentation"
                  />
                  <path
                    d="M0 10 a10 10 0 0 1 10 -10 h{cw - 20} a10 10 0 0 1 10 10 v20 h-{cw} z"
                    fill={col} fill-opacity={sel ? 0.45 : 0.3}
                    class="pointer-events-none"
                  />
                  <text x="14" y="20" font-size="12" font-weight="600"
                    class="pointer-events-none select-none fill-foreground/80">
                    {String(n.config.text ?? 'Comment').trim() || 'Comment'}
                  </text>
                  <polygon
                    points="{cw - 16},{ch} {cw},{ch - 16} {cw},{ch}"
                    fill={col} fill-opacity="0.7"
                    class="cursor-se-resize"
                    onpointerdown={(ev) => { selectedNodeId = n.id; startCommentResize(ev, n); }}
                    role="presentation"
                  />
                </g>
              {/each}

              <!-- edges -->
              {#each graph.edges as e, idx (edgeViaKey(e))}
                {@const fromN = nodeById.get(e.from)}
                {@const toN = nodeById.get(e.to)}
                {#if fromN && toN}
                  {@const x1 = fromN.x + NODE_W}
                  {@const y1 = outPortY(fromN, e.from_port)}
                  {@const x2 = toN.x}
                  {@const y2 = inPortY(toN, e.to_port)}
                  {@const eLive = nodeLive(e.to)}
                  {@const eColor = portColorOf(fromN, e.from_port, "out")}
                  {@const vk = edgeViaKey(e)}
                  {@const vias = edgeVias[vk] ?? []}
                  {@const pathD = (() => {
                    if (vias.length === 0) return `M ${x1} ${y1} C ${x1+60} ${y1}, ${x2-60} ${y2}, ${x2} ${y2}`;
                    const pts = [{x:x1,y:y1}, ...vias, {x:x2,y:y2}];
                    let d = `M ${pts[0].x} ${pts[0].y}`;
                    for (let pi = 0; pi < pts.length - 1; pi++) {
                      const a = pts[pi], b = pts[pi+1];
                      const c1x = pi === 0 ? a.x+60 : a.x+(b.x-a.x)*0.35;
                      const c1y = pi === 0 ? a.y : a.y+(b.y-a.y)*0.1;
                      const c2x = pi === pts.length-2 ? b.x-60 : b.x-(b.x-a.x)*0.35;
                      const c2y = pi === pts.length-2 ? b.y : b.y-(b.y-a.y)*0.1;
                      d += ` C ${c1x} ${c1y}, ${c2x} ${c2y}, ${b.x} ${b.y}`;
                    }
                    return d;
                  })()}
                  <!-- click+drag anywhere on the wire to add/move a bend (up to 2);
                       alt+click on wire deletes it; alt+click on a dot straightens that segment -->
                  <g
                    role="presentation"
                    onpointerenter={() => { hoveredEdgeKey = vk; }}
                    onpointerleave={() => { if (hoveredEdgeKey === vk) hoveredEdgeKey = null; }}
                  >
                    <!-- wide transparent hit area; alt+click deletes wire; drag adds/moves bend -->
                    <path
                      d={pathD}
                      fill="none"
                      stroke="transparent"
                      stroke-width="20"
                      class="cursor-crosshair"
                      onpointerdown={(ev) => {
                        ev.stopPropagation();
                        if (ev.altKey) { removeEdge(idx); return; }
                        if (vias.length < 2) {
                          // Add a new via at the click point, sorted by x so segments stay ordered.
                          const svgPt = worldPoint(ev);
                          const newVia = { x: svgPt.x, y: svgPt.y };
                          const cur = [...vias, newVia].sort((a, b) => a.x - b.x);
                          edgeVias[vk] = cur;
                          dragVia = { key: vk, idx: cur.indexOf(newVia) };
                        } else {
                          // Already 2 vias — drag the nearest one instead of adding.
                          const svgPt = worldPoint(ev);
                          const nearIdx = vias.reduce((bi, v, vi) =>
                            Math.hypot(v.x-svgPt.x, v.y-svgPt.y) < Math.hypot(vias[bi].x-svgPt.x, vias[bi].y-svgPt.y) ? vi : bi, 0);
                          dragVia = { key: vk, idx: nearIdx };
                        }
                      }}
                      role="presentation"
                    />
                    <!-- visible wire; turns red on hover -->
                    <path
                      d={pathD}
                      fill="none"
                      stroke={eColor}
                      stroke-width={eLive ? 3 : 2}
                      stroke-opacity={eLive ? 1 : 0.55}
                      class="pointer-events-none transition-colors {hoveredEdgeKey === vk ? 'stroke-destructive/70' : ''} {eLive ? 'node-running' : ''}"
                      role="presentation"
                    />
                    <!-- bend handles; alt+click removes that via-point -->
                    {#each vias as via, vi}
                      <circle
                        cx={via.x}
                        cy={via.y}
                        r="5"
                        fill={eColor}
                        stroke="white"
                        stroke-width="1.5"
                        stroke-opacity="0.4"
                        class="cursor-grab"
                        onpointerdown={(ev) => {
                          ev.stopPropagation();
                          if (ev.altKey) {
                            edgeVias[vk] = vias.filter((_, j) => j !== vi);
                            if (edgeVias[vk].length === 0) delete edgeVias[vk];
                            return;
                          }
                          dragVia = { key: vk, idx: vi };
                        }}
                        role="presentation"
                      />
                    {/each}
                  </g>
                {/if}
              {/each}

              <!-- regular nodes (comment boxes rendered in the pass above) -->
              {#each nonCommentNodes as n (n.id)}
                {@const k = effectiveKind(n)}
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
                    class="node-body transition-[stroke] {nodeBorderClass(n)}"
                  />
                  {#if live}
                    <g transition:fade={{ duration: 200 }}>
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
                    </g>
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
                    {nodeTitle(n, k)}
                  </text>
                  <line
                    x1="0"
                    y1={HEAD_H}
                    x2={NODE_W}
                    y2={HEAD_H}
                    class="pointer-events-none stroke-border"
                    stroke-width="1"
                  />
                  {#if isCompareOrLogic(n.kind)}
                    {@const op = opLabel(n.kind, n.config)}
                    {@const aVal = exprOperand(n.id, "a", n.config)}
                    {@const bVal = exprOperand(n.id, "b", n.config)}
                    {@const ey = HEAD_H + EXPR_H / 2}
                    <text
                      x={NODE_W / 2}
                      y={ey + 7}
                      text-anchor="middle"
                      font-size="17"
                      font-weight="700"
                      class="pointer-events-none select-none fill-primary"
                    >{op}</text>
                    <text
                      x="10"
                      y={ey + 5}
                      font-size="8"
                      class="pointer-events-none select-none fill-muted-foreground/70"
                    >{aVal}</text>
                    <text
                      x={NODE_W - 10}
                      y={ey + 5}
                      text-anchor="end"
                      font-size="8"
                      class="pointer-events-none select-none fill-muted-foreground/70"
                    >{bVal}</text>
                  {/if}
                  {#if invalid}
                    <!-- required input(s) not filled — a red dot, the logo's signal -->
                    <circle cx={NODE_W - 9} cy="9" r="5.5" class="pointer-events-none fill-destructive/20" />
                    <circle cx={NODE_W - 9} cy="9" r="3" class="fill-destructive">
                      <title>Required input not filled: {requiredUnfilled(n).map((p) => p.name).join(", ")}</title>
                    </circle>
                  {/if}

                  <!-- exec (control-flow) pins — white. exec_in on the left header.
                       Pure data/logic nodes (comparisons, conversions) have no exec pins. -->
                  {#if !isPure(k)}
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
                  {/if}
                  <!-- exec outputs: single exec_out stays at the header right edge;
                       multi-exec (arms/branches) all live in the node body so the header
                       area stays clean and the arms read left-to-right with their labels. -->
                  {#each execOutPorts(k) as p, i}
                    {@const multi = hasMultipleExecOuts(k)}
                    {@const ey = multi ? HEAD_H + i * ROW_H + ROW_H / 2 : HEAD_H / 2}
                    {@const isP = pending && pending.node === n.id && pending.port === p.name}
                    <circle
                      cx={NODE_W}
                      cy={ey}
                      r={isP ? 6 : 5}
                      fill={isP ? portColor.exec : "#1a2420"}
                      stroke={portColor.exec}
                      stroke-width="2"
                      class="cursor-pointer transition-all"
                      onclick={() => clickOutPort(n, p.name)}
                      role="presentation"
                    />
                    {#if multi}
                      <text
                        x={NODE_W - 9}
                        y={ey + 4}
                        text-anchor="end"
                        class="pointer-events-none select-none fill-muted-foreground"
                        font-size="9"
                      >
                        {execPortLabel(n, p.name)}
                      </text>
                    {/if}
                  {/each}

                  <!-- input data ports -->
                  {#each dataPorts(k, "in") as p, i}
                    {@const wired = wiredInto(n.id, p.name)}
                    {@const need = p.required && !isFilled(n, p)}
                    {@const py = dataPortY(i) + nodeExprH(n)}
                    <circle
                      cx="0"
                      cy={py}
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
                      y={py + 3}
                      class="pointer-events-none select-none {need ? 'fill-destructive' : 'fill-muted-foreground'}"
                      font-size="10"
                    >
                      {p.name}{p.required ? "*" : ""}
                    </text>
                  {/each}

                  <!-- output data ports (stacked below the exec-out pins) -->
                  {#each dataPorts(k, "out") as p, i}
                    {@const isPending = pending && pending.node === n.id && pending.port === p.name}
                    {@const dy = dataPortY(i + outDataOffset(k)) + nodeExprH(n)}
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

                  <!-- TimeGate live countdown — shown when the node is armed -->
                  {#if n.kind === "time_gate"}
                    {@const secs = timeGateCountdowns[n.id]}
                    {@const armed = secs != null}
                    {@const label = armed ? `${secs}s remaining` : "idle"}
                    <rect
                      x={8}
                      y={portsBottom(n) + 4}
                      width={NODE_W - 16}
                      height={18}
                      rx="4"
                      class={armed ? "fill-primary/15" : "fill-muted/20"}
                    />
                    <text
                      x={NODE_W / 2}
                      y={portsBottom(n) + 16}
                      text-anchor="middle"
                      class={armed ? "fill-primary" : "fill-muted-foreground"}
                      font-size="10"
                      font-weight={armed ? "600" : "400"}
                    >{label}</text>
                  {/if}

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
                {@const pn = nodeById.get(pending.node)}
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
              onclick={(e) => { contextPos = { x: (e.target as HTMLElement).getBoundingClientRect().left, y: 60 }; contextPending = null; contextOpen = true; }}
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
                          {@const cv = cameraValue(String(selectedNode.config[key] ?? ""))}
                          {@const clabel = cameraOptions.find((o) => o.value === cv)?.label ?? cv}
                          <Select.Root type="single" disabled={editLocked} value={cv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? cv })}>
                            <Select.Trigger class="h-8 w-full">{clabel}</Select.Trigger>
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
                      {:else if key === "room"}
                        {#if rooms.length === 0}
                          <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">
                            No rooms — add zones on the Map.
                          </p>
                        {:else}
                          {@const rmStored = String(selectedNode.config[key] ?? "")}
                          {@const rmv = rmStored === "" ? NO_ROOM : rmStored}
                          {@const rmlabel = roomOptions.find((o) => o.value === rmv)?.label ?? rmStored}
                          <Select.Root type="single" disabled={editLocked} value={rmv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v === NO_ROOM ? "" : (v ?? "") })}>
                            <Select.Trigger class="h-8 w-full">{rmlabel}</Select.Trigger>
                            <Select.Content>
                              {#each roomOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if key === "category"}
                        {@const cv2 = String(selectedNode.config[key] ?? "person")}
                        {@const clabel2 = categoryOptions.find((o) => o.value === cv2)?.label ?? cv2}
                        <Select.Root type="single" disabled={editLocked} value={cv2} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "person" })}>
                          <Select.Trigger class="h-8 w-full">{clabel2}</Select.Trigger>
                          <Select.Content>
                            {#each categoryOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else if key === "class" && selectedNode.kind === "object_detection"}
                        {@const clsWired = graph.edges.some((e) => e.to === selectedNode.id && e.to_port === "class")}
                        {@const clsv = String(selectedNode.config[key] ?? "person")}
                        <SearchableSelect
                          value={clsv}
                          options={COCO_CLASSES}
                          disabled={editLocked || clsWired}
                          onchange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, class: v })}
                        />
                        {#if clsWired}
                          <p class="mt-0.5 text-[10px] text-muted-foreground">Overridden by wired input</p>
                        {/if}
                      {:else if key === "operator" && selectedNode.kind === "text_compare"}
                        {@const TEXT_OPS = [
                          { value: "eq", label: "equals" },
                          { value: "ne", label: "not equals" },
                          { value: "contains", label: "contains" },
                          { value: "starts_with", label: "starts with" },
                          { value: "ends_with", label: "ends with" },
                        ]}
                        {@const topv = String(selectedNode.config[key] ?? "eq")}
                        {@const toplabel = TEXT_OPS.find((o) => o.value === topv)?.label ?? topv}
                        <Select.Root type="single" disabled={editLocked} value={topv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "eq" })}>
                          <Select.Trigger class="h-8 w-full">{toplabel}</Select.Trigger>
                          <Select.Content>
                            {#each TEXT_OPS as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else if key === "operator"}
                        {@const ops = [">", ">=", "<", "<=", "==", "!="]}
                        {@const opv = String(selectedNode.config[key] ?? "==")}
                        <Select.Root type="single" disabled={editLocked} value={opv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "==" })}>
                          <Select.Trigger class="h-8 w-full font-mono">{opv}</Select.Trigger>
                          <Select.Content>
                            {#each ops as op (op)}<Select.Item value={op} label={op} class="font-mono">{op}</Select.Item>{/each}
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
                          onchange={(e) => chooseEvent(selectedNode, e.currentTarget.value)}
                        />
                        <datalist id="wf-events">
                          {#each events as e (e.name)}<option value={e.name}>{e.category}</option>{/each}
                        </datalist>
                        {#if evName && known}
                          <p class="mt-1 text-[10px] text-muted-foreground">Registered · “{known.category}”</p>
                        {/if}
                      {:else if key === "style" && selectedNode.kind === "draw_bbox"}
                        {@const STYLES = [
                          { value: "box", label: "Box (rectangle)" },
                          { value: "corners", label: "Corner brackets" },
                          { value: "reticle", label: "Targeting reticle" },
                        ]}
                        {@const stv = String(selectedNode.config[key] ?? "box")}
                        {@const stlabel = STYLES.find((o) => o.value === stv)?.label ?? stv}
                        <Select.Root type="single" disabled={editLocked} value={stv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "box" })}>
                          <Select.Trigger class="h-8 w-full">{stlabel}</Select.Trigger>
                          <Select.Content>
                            {#each STYLES as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else if key === "color"}
                        <input
                          type="color"
                          class="h-8 w-full cursor-pointer rounded-lg border border-border bg-card"
                          disabled={editLocked}
                          value={String(selectedNode.config[key] ?? '#facc15')}
                          oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)}
                          onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })}
                        />
                      {:else}
                        <Input class="h-8" disabled={editLocked} value={String(selectedNode.config[key] ?? "")} oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)} />
                      {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
              <!-- Event payload form: category, the typed fields (label on the left like every
                   other row), and a Save at the bottom. Saving updates the registry; graphs
                   using this event self-heal when next opened. -->
              {#if selectedNode.kind === "emit_event" || selectedNode.kind === "on_event"}
                {@const evName = String(selectedNode.config.event ?? "").trim()}
                {@const knownEv = events.find((e) => e.name === evName)}
                {#if evName && knownEv && knownEv.source === "system"}
                  <!-- Platform-defined event: read-only (its payload schema ships with the
                       app, available to every tenant — no registration needed). -->
                  <div class="space-y-1 border-t border-border pt-2.5">
                    <p class="text-[10px] font-medium text-muted-foreground">System event · “{knownEv.category}” (read-only)</p>
                    {#if (knownEv.fields ?? []).length > 0}
                      <p class="text-[10px] text-muted-foreground">Payload: {(knownEv.fields ?? []).map((f) => `${f.name} (${f.type})`).join(", ")}</p>
                    {/if}
                  </div>
                {:else if evName}
                  <div class="space-y-2.5 border-t border-border pt-2.5">
                    <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
                      <Label class="pt-2 text-xs text-muted-foreground">Category</Label>
                      <div>
                        <Input class="h-8" list="wf-event-cats" disabled={editLocked} placeholder="General" bind:value={newEventCategory} />
                        <datalist id="wf-event-cats">{#each eventCategories as c (c.name)}<option value={c.name}></option>{/each}</datalist>
                      </div>
                    </div>
                    <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
                      <Label class="pt-2 text-xs text-muted-foreground">Payload</Label>
                      <div class="space-y-1.5">
                        {#each newEventFields as f, i (i)}
                          <div class="flex items-center gap-1.5">
                            <Input class="h-8 min-w-0" disabled={editLocked} placeholder="field (e.g. Message)" value={f.name} oninput={(e) => setEventFieldName(i, e.currentTarget.value)} />
                            <Select.Root type="single" disabled={editLocked} value={f.type} onValueChange={(v) => setEventFieldType(i, (v ?? "text") as PortType)}>
                              <Select.Trigger class="h-8 w-24 shrink-0">{payloadTypeLabel(f.type)}</Select.Trigger>
                              <Select.Content>
                                {#each PAYLOAD_TYPES as t (t.value)}<Select.Item value={t.value} label={t.label}>{t.label}</Select.Item>{/each}
                              </Select.Content>
                            </Select.Root>
                            <Button size="icon" variant="ghost" class="h-8 w-8 shrink-0" disabled={editLocked} onclick={() => removeEventField(i)} title="Remove field">×</Button>
                          </div>
                        {/each}
                        <Button size="sm" variant="outline" class="h-8 w-full" disabled={editLocked} onclick={addEventField}>+ Payload field</Button>
                      </div>
                    </div>
                    <Button size="sm" class="h-8 w-full" disabled={editLocked} onclick={() => registerEvent(evName)}>Save</Button>
                    <p class="text-[10px] text-muted-foreground">
                      {#if knownEv}Saved · “{knownEv.category}”. Saving updates graphs using this event when they next open.{:else}New event — Save so other graphs can listen.{/if}
                    </p>
                  </div>
                {/if}
              {/if}
              <!-- String switch: user-defined match arms. Each arm becomes an exec output
                   port. A built-in "default" arm always fires when no arm matches. -->
              {#if selectedNode.kind === "string_switch"}
                {@const swArms = Array.isArray(selectedNode.config?.arms) ? (selectedNode.config.arms as string[]) : []}
                <div class="space-y-2.5 border-t border-border pt-2.5">
                  <p class="text-xs font-medium text-muted-foreground">Match arms</p>
                  <div class="space-y-1.5">
                    {#each swArms as arm, i (i)}
                      <div class="flex items-center gap-1.5">
                        <Input
                          class="h-8 min-w-0 flex-1"
                          disabled={editLocked}
                          placeholder="arm label (e.g. red)"
                          value={arm}
                          oninput={(e) => {
                            const updated = [...swArms];
                            updated[i] = e.currentTarget.value;
                            saveNodeConfig(selectedNode, { ...selectedNode.config, arms: updated });
                          }}
                        />
                        <Button
                          size="icon"
                          variant="ghost"
                          class="h-8 w-8 shrink-0"
                          disabled={editLocked || swArms.length <= 1}
                          onclick={() => {
                            const updated = swArms.filter((_, j) => j !== i);
                            saveNodeConfig(selectedNode, { ...selectedNode.config, arms: updated });
                          }}
                          title="Remove arm"
                        >×</Button>
                      </div>
                    {/each}
                    <Button
                      size="sm"
                      variant="outline"
                      class="h-8 w-full"
                      disabled={editLocked}
                      onclick={() => {
                        const updated = [...swArms, `arm_${swArms.length + 1}`];
                        saveNodeConfig(selectedNode, { ...selectedNode.config, arms: updated });
                      }}
                    >+ Add arm</Button>
                  </div>
                  <p class="text-[10px] text-muted-foreground">A built-in <strong>default</strong> arm always fires when no label matches.</p>
                </div>
              {/if}
              <!-- Camera switch: user-defined camera arms. Each arm maps a camera (via dropdown)
                   to a numbered exec output port. A built-in "default" arm fires when no camera
                   matches. -->
              {#if selectedNode.kind === "camera_switch"}
                {@const cwArms = Array.isArray(selectedNode.config?.arms) ? (selectedNode.config.arms as string[]) : []}
                <div class="space-y-2.5 border-t border-border pt-2.5">
                  <p class="text-xs font-medium text-muted-foreground">Camera arms</p>
                  <div class="space-y-1.5">
                    {#each cwArms as arm, i (i)}
                      <div class="flex items-center gap-1.5">
                        {#if cameraOptions.length === 0}
                          <p class="flex-1 rounded border border-dashed border-border px-2 py-1 text-xs text-muted-foreground">No cameras</p>
                        {:else}
                          {@const cv = cameraValue(arm)}
                          {@const clabel = cameraOptions.find((o) => o.value === cv)?.label ?? cv}
                          <Select.Root
                            type="single"
                            disabled={editLocked}
                            value={cv}
                            onValueChange={(v) => {
                              const updated = [...cwArms];
                              updated[i] = v ?? cv;
                              saveNodeConfig(selectedNode, { ...selectedNode.config, arms: updated });
                            }}
                          >
                            <Select.Trigger class="h-8 flex-1">{clabel}</Select.Trigger>
                            <Select.Content>
                              {#each cameraOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                        <Button
                          size="icon"
                          variant="ghost"
                          class="h-8 w-8 shrink-0"
                          disabled={editLocked || cwArms.length <= 1}
                          onclick={() => {
                            const updated = cwArms.filter((_, j) => j !== i);
                            saveNodeConfig(selectedNode, { ...selectedNode.config, arms: updated });
                          }}
                          title="Remove arm"
                        >×</Button>
                      </div>
                    {/each}
                    <Button
                      size="sm"
                      variant="outline"
                      class="h-8 w-full"
                      disabled={editLocked}
                      onclick={() => {
                        const updated = [...cwArms, cameraOptions[0]?.value ?? ""];
                        saveNodeConfig(selectedNode, { ...selectedNode.config, arms: updated });
                      }}
                    >+ Add arm</Button>
                  </div>
                  <p class="text-[10px] text-muted-foreground">A built-in <strong>default</strong> arm fires when no camera matches.</p>
                </div>
              {/if}
              <!-- Memoize: cache the node's output for the run (default) vs recompute each
                   time it's reached. Turn OFF for loop bodies / live sources so loops iterate.
                   Hidden for event nodes — they're no-op signals that never memoize. -->
              {#if selectedNode.kind !== "emit_event" && selectedNode.kind !== "on_event" && selectedNode.kind !== "comment"}
                <div class="flex items-center justify-between gap-2 border-t border-border pt-2.5">
                  <Label
                    class="flex items-center gap-1.5 text-xs text-muted-foreground"
                    title="On: cache this node's output for the run and reuse it. Off: recompute every time the node runs — required for loop bodies and live sources (e.g. Get current frame)."
                  >
                    Memoize output
                  </Label>
                  <Switch
                    checked={selectedNode.memoize === true}
                    disabled={editLocked}
                    onCheckedChange={(v) => saveNodeMemoize(selectedNode, v)}
                  />
                </div>
              {/if}
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
          <div class="overflow-y-auto border-t border-border bg-well p-4 font-mono text-sm" style="height: {consoleH}px">
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

<ContextSearch
  bind:open={contextOpen}
  pos={contextPos}
  portType={contextPortType}
  kinds={kinds as PaletteKind[]}
  onSelect={addFromContext}
/>

<!-- Recent runs drawer — persisted run history for one graph (trigger, pod, status). -->
<Dialog.Root bind:open={runsOpen}>
  <Dialog.Content class="max-w-2xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <History class="size-4" /> Recent runs — {runsGraph?.name ?? ""}
        {#if runsGraph}
          <a
            href="/logs?module=workflow&entity_type=workflow_graph&entity_id={runsGraph.id}"
            class="ml-auto text-[11px] text-primary hover:underline"
            onclick={() => (runsOpen = false)}
          >View all in Logs →</a>
        {/if}
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
              <span class="ml-auto shrink-0 text-[10px] text-muted-foreground">{fmtTs(r.started_at)}</span>
              <span class="shrink-0 text-[10px] text-muted-foreground/70">· {r.pod_name}</span>
            </button>
            {#if runsExpanded === r.id}
              <div class="border-t border-border px-3 py-2">
                {#if r.result_summary}<p class="mb-1.5 text-[11px] text-muted-foreground">{r.result_summary}</p>{/if}
                {#if (runsLogs[r.id]?.length ?? 0) === 0}
                  <p class="text-[11px] text-muted-foreground/60">No detailed signals recorded.</p>
                {:else}
                  <div class="space-y-0.5 font-mono text-[10px] leading-relaxed">
                    {#each runsLogs[r.id] as line (line.id)}
                      <div class="flex gap-2">
                        <span class="w-10 shrink-0 uppercase {line.level === 'error' ? 'text-destructive' : line.level === 'warn' ? 'text-amber-400' : 'text-muted-foreground/60'}">{line.level}</span>
                        <span class="{line.level === 'error' ? 'text-destructive' : 'text-muted-foreground'}">
                          {(line.payload?.label as string | undefined) ? `${line.payload?.label}: ` : ""}{line.message}
                        </span>
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

<style>
  /* Node body fill — slightly elevated above the canvas so nodes read as distinct surfaces.
     --card alone is too dark; mixing with --popover lifts it ~2 tonal steps. */
  :global(.node-body) {
    fill: color-mix(in srgb, var(--card) 55%, var(--popover) 45%);
  }
</style>
