<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { page } from "$app/stores";
  import { goto, beforeNavigate } from "$app/navigation";
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
  import Save from "@lucide/svelte/icons/save";
  import Pause from "@lucide/svelte/icons/pause";
  import Square from "@lucide/svelte/icons/square";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Power from "@lucide/svelte/icons/power";
  import Busy from "$lib/components/busy.svelte";
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
  import Copy from "@lucide/svelte/icons/copy";
  import CopyPlus from "@lucide/svelte/icons/copy-plus";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import Workflow from "@lucide/svelte/icons/workflow";
  import Table2 from "@lucide/svelte/icons/table-2";
  import Link2 from "@lucide/svelte/icons/link-2";
  import Unplug from "@lucide/svelte/icons/unplug";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import Hand from "@lucide/svelte/icons/hand";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Upload from "@lucide/svelte/icons/upload";
  import ImageIcon from "@lucide/svelte/icons/image";
  import FileIcon from "@lucide/svelte/icons/file";
  import Bug from "@lucide/svelte/icons/bug";
  import Printer from "@lucide/svelte/icons/printer";
  import History from "@lucide/svelte/icons/history";
  import Layers from "@lucide/svelte/icons/layers";
  // Node-category glyphs — give each node family a face on the canvas.
  import Camera from "@lucide/svelte/icons/camera";
  import Eye from "@lucide/svelte/icons/eye";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Bell from "@lucide/svelte/icons/bell";
  import Send from "@lucide/svelte/icons/send";
  import Type from "@lucide/svelte/icons/type";
  import Globe from "@lucide/svelte/icons/globe";
  import { auth } from "$lib/auth.svelte";
  import Waypoints from "@lucide/svelte/icons/waypoints";
  import Zap from "@lucide/svelte/icons/zap";
  import Radar from "@lucide/svelte/icons/radar";
  import Variable from "@lucide/svelte/icons/variable";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Box from "@lucide/svelte/icons/box";
  import Binary from "@lucide/svelte/icons/binary";
  import Disc from "@lucide/svelte/icons/disc";
  import CircleStop from "@lucide/svelte/icons/circle-stop";
  import Folder from "@lucide/svelte/icons/folder";
  import DictionaryEditor from "$lib/components/dictionary-editor.svelte";
  import SearchableSelect from "$lib/components/searchable-select.svelte";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import { Slider } from "$lib/components/ui/slider";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Select from "$lib/components/ui/select";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import OlharMark from "$lib/components/olhar-mark.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";
  import {
    portColor,
    CATEGORY_COLOR,
    EVENT_HEADER_COLOR,
    DEFAULT_CATEGORY_COLOR,
    FALLBACK_PORT_COLOR,
    COMMENT_DEFAULT_COLOR,
  } from "$lib/workflows/portColors";
  import type { PortType } from "$lib/workflows/portColors";
  import { titleFontSize as dTitleFontSize, titleWidth, titleHeight, wrapText, mdLines } from "$lib/workflows/display";
  import { triggerSourceLabel } from "$lib/workflows/format";

  import ContextSearch from "../components/ContextSearch.svelte";
  import WorkflowsList from "../components/WorkflowsList.svelte";
  import VariablesPanel from "../components/VariablesPanel.svelte";
  import DictionaryModal from "$lib/components/dictionary-modal.svelte";
  import RegionPointsEditor from "../components/RegionPointsEditor.svelte";

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
  type ScheduleFreq = "hourly" | "daily" | "weekly" | "monthly" | "yearly";
  type ScheduleT = {
    freq: ScheduleFreq;
    tz: string;
    times: string[];
    minute: number;
    weekdays: number[]; // 0=Mon … 6=Sun
    day_of_month: number;
    month: number; // 1-12
  };
  type Trigger = "Manual" | { Cron: { every_secs: number } } | { Schedule: ScheduleT };
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
  // Variable types (the Variables panel). Mirrors PAYLOAD_TYPES but "Dictionary" here means a
  // reference to a named typed-column table (PortType::Table, #113), not the flat string→string
  // dictionary that HTTP Post's headers/fields use.
  const VAR_TYPES: { value: string; label: string }[] = [
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
    { value: "table", label: "Dictionary Table" },
  ];
  // A palette entry: a node kind, optionally pre-bound to an event name (for the
  // synthesized "On event: Foo" / "Emit event: Foo" entries).
  type PaletteKind = CatKind & { presetEvent?: string; presetVariable?: { name: string; type: string } };
  type RunRecord = {
    id: string;
    graph_id: string;
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
  type Variable = { name: string; type: PortType; default?: string | null };
  type Graph = {
    id: string;
    name: string;
    description?: string;
    group?: string;
    enabled: boolean;
    trigger: Trigger;
    concurrency: Concurrency;
    instance_scope?: "Single" | "PerCamera"; // what the graph runs one of (#95)
    run_orphans?: boolean;
    debug?: boolean;
    nodes: Node[];
    edges: Edge[];
    next_id?: number; // server-assigned next node id; client mints ids from here between saves (#122)
    viewport?: { x: number; y: number; zoom: number };
    edge_vias?: Record<string, [number, number][]>;
    variables?: VarDecl[];
    template?: boolean;
    global?: boolean;
    last_run?: { at: string; status: string; trigger: string } | null;
  };
  // A graph variable. `param` marks it as a user-facing INPUT (the UE "expose" flag); `description`
  // is its help text in the setup form. Both additive — old graphs deserialize without them.
  type VarDecl = {
    name: string;
    type: string;
    default?: string | null;
    param?: boolean;
    description?: string | null;
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

  // ---- display / annotation nodes (Title, Paragraph, Markdown, Tabs) -------
  // Rendered in the decoration layer (like Comment) with custom SVG bodies — no ports, no exec.
  // Reuse startDrag / startCommentResize / isSelected; sized by `dispW`/`dispH` below.
  const DISPLAY_KINDS = ["title", "paragraph", "markdown"];
  function isDisplayCard(kind: string): boolean { return DISPLAY_KINDS.includes(kind); }
  function isDecoration(kind: string): boolean { return kind === "comment" || isDisplayCard(kind); }
  // Title sizing wrappers over the pure helpers in $lib/workflows/display (dispW/dispH/render use these).
  function titleFontSize(n: Node): number { return dTitleFontSize(n.config.size); }
  function titleW(n: Node): number { return titleWidth(String(n.config.text ?? "Title"), dTitleFontSize(n.config.size)); }
  function titleH(n: Node): number { return titleHeight(dTitleFontSize(n.config.size)); }
  // Rendered box size for any decoration node (bounds / marquee / drag).
  function dispW(n: Node): number {
    switch (n.kind) {
      case "comment": case "paragraph": case "markdown": return commentW(n);
      case "title": return titleW(n);
      default: return nodeW(n);
    }
  }
  function dispH(n: Node): number {
    switch (n.kind) {
      case "comment": case "paragraph": case "markdown": return commentH(n);
      case "title": return titleH(n);
      default: return nodeHeight(n);
    }
  }

  // AABB overlap: does node `n`'s box intersect the marquee box `b` (world coords)?
  function nodeInBox(n: Node, b: { x0: number; y0: number; x1: number; y1: number }): boolean {
    const w = isDecoration(n.kind) ? dispW(n) : nodeW(n);
    const h = isDecoration(n.kind) ? dispH(n) : nodeHeight(n);
    return n.x < b.x1 && n.x + w > b.x0 && n.y < b.y1 && n.y + h > b.y0;
  }
  const PREVIEW_H = Math.round((PREVIEW_W * 9) / 16);

  // ---- state ---------------------------------------------------------------
  let graphs = $state<Graph[]>([]);
  let catalog = $state<Record<string, CatKind>>({});
  let view = $state<"list" | "editor">("list");
  let selectedGraphId = $state<string | null>(null);
  let selectedNodeId = $state<number | null>(null);
  let pending = $state<{ node: number; port: string } | null>(null);
  // Multi-selection: `selectedNodeId` stays the PRIMARY (drives the inspector); `selectedIds`
  // is the full set (marquee + shift-click). A node is highlighted if it's in either.
  let selectedIds = $state<Set<number>>(new Set());
  const isSelected = (id: number) => selectedIds.has(id) || selectedNodeId === id;
  // Rubber-band marquee (world coords) while drag-selecting on empty canvas in move mode.
  let marquee = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  // Copy/paste buffer: deep-cloned nodes (+ their internal edges) from the last copy/duplicate.
  let clipboard = $state<{ nodes: Node[]; edges: Edge[] }>({ nodes: [], edges: [] });
  // Select exactly one node (replace the set); shift-click toggles instead (see onNodeSelect).
  function selectOnly(id: number) {
    selectedNodeId = id;
    selectedIds = new Set([id]);
  }
  function clearSelection() {
    selectedNodeId = null;
    selectedIds = new Set();
  }
  // ---- explicit Save (#122): edits are LOCAL + instant; nothing persists until Save --------------
  let dirty = $state(false);
  let saving = $state(false);
  function markDirty() { dirty = true; }
  // A real (non-negative) client-assigned node id for a new local node (no server round-trip).
  function nextNodeId(g: Graph): number {
    const n = typeof g.next_id === "number" && g.next_id > 0 ? g.next_id : Math.max(0, ...g.nodes.map((x) => x.id)) + 1;
    g.next_id = n + 1;
    return n;
  }
  async function save() {
    if (!graph || saving) return;
    saving = true;
    try {
      await data.saveGraph(graph.id, {
        nodes: $state.snapshot(graph.nodes),
        edges: $state.snapshot(graph.edges),
        viewport: $state.snapshot(graph.viewport),
        edge_vias: $state.snapshot(graph.edge_vias),
        next_id: graph.next_id,
      });
      dirty = false;
    } catch (e) {
      toast.error("Couldn't save workflow", { description: String(e) });
    } finally {
      saving = false;
    }
  }
  // Backstop for leaving the editor by any OTHER path (a sidebar link, a browser back). Internal
  // graph switches (openGraph/backToList) already save first, so `dirty` is false by the time this
  // fires for them. A fire-and-forget save keeps the canvas from being lost. (#122)
  beforeNavigate(() => {
    if (dirty && !saving) void save();
  });
  // Editor tool/mode (touch-first): move = drag nodes, pan = drag canvas, link = tap ports to
  // wire (node body inert), break = tap a wire to delete it. Default preserves mouse behavior.
  let editorMode = $state<"move" | "pan" | "link" | "break">("move");
  // Coarse pointer (tablet/touch) → bigger hit targets. Set on mount (browser-only).
  let isTouch = $state(false);
  const portR = $derived(isTouch ? 8 : 5); // port circle radius — fatter for touch
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
  function toggleConsole() {
    consoleOpen = !consoleOpen;
    preToggledPanels = null; // a manual toggle ends the collapsed group
    try {
      localStorage.setItem("wf.consoleOpen", String(consoleOpen));
    } catch {
      /* ignore */
    }
  }
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

  async function loadConsoleLogs(graphId: string) {
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
  // Display live sinks (Text/Gauge): the latest value that flowed into each node, by node id.
  // Persisted node-state, so the readout survives a refresh; unset → the node shows its config literal.
  let nodeValues = $state<Record<number, unknown>>({});
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

  // The right rail — one tabbed sidebar holding every side panel in the editor
  // (Details / Properties / Variables / Runs). There is no left rail: variables moved
  // in here, which gives the canvas its width back.
  const PANEL_MIN = 240;
  const PANEL_MAX = 560;
  type RailTab = "details" | "properties" | "variables" | "runs";
  let panelW = $state(300);
  let panelOpen = $state(true);
  let panelTab = $state<RailTab>("properties");
  let resizing = $state(false);
  function togglePanel() {
    panelOpen = !panelOpen;
    preToggledPanels = null; // a manual toggle ends the collapsed group
    try {
      localStorage.setItem("wf.panelOpen", String(panelOpen));
    } catch {
      /* ignore */
    }
  }

  // Backtick (`) collapses the editor's chrome — the right rail and the console — as ONE group,
  // so a single keypress hands the canvas the whole window. Pressing it again restores each panel
  // to exactly what it was: a panel you'd already closed by hand stays closed. Any manual toggle
  // clears the memory, so the next press is a fresh collapse rather than a partial restore.
  let preToggledPanels: { rail: boolean; console: boolean } | null = null;
  function toggleChrome() {
    if (preToggledPanels) {
      panelOpen = preToggledPanels.rail;
      consoleOpen = preToggledPanels.console;
      preToggledPanels = null;
    } else if (!panelOpen && !consoleOpen) {
      // Nothing left to collapse — the toggle would look dead on a bare canvas. Open the rail.
      panelOpen = true;
    } else {
      preToggledPanels = { rail: panelOpen, console: consoleOpen };
      panelOpen = false;
      consoleOpen = false;
    }
    try {
      localStorage.setItem("wf.panelOpen", String(panelOpen));
      localStorage.setItem("wf.consoleOpen", String(consoleOpen));
    } catch {
      /* ignore */
    }
  }
  function selectTab(t: RailTab) {
    panelTab = t;
    panelOpen = true; // picking a tab on the collapsed rail reopens it
    preToggledPanels = null; // …which also ends the collapsed group
    try {
      localStorage.setItem("wf.panelTab", t);
      localStorage.setItem("wf.panelOpen", "true");
    } catch {
      /* ignore */
    }
    // Clicking Runs always re-fetches, so you see the run you just kicked off. Claiming
    // the graph here also stops the mount-restore effect below from fetching a second time.
    if (t === "runs" && graph) {
      runsLoadedFor = graph.id;
      loadRuns(graph);
    }
  }
  // The rail remembers its tab, so the editor can open straight onto Runs — in which case
  // selectTab() never fired and nothing would have loaded the history.
  let runsLoadedFor: string | null = null;
  $effect(() => {
    if (panelTab !== "runs" || !graph || runsLoadedFor === graph.id) return;
    runsLoadedFor = graph.id;
    loadRuns(graph);
  });
  const RAIL_TABS: { id: RailTab; label: string; icon: typeof Settings2 }[] = [
    { id: "details", label: "Details", icon: Settings2 },
    { id: "properties", label: "Properties", icon: SlidersHorizontal },
    { id: "variables", label: "Variables", icon: Variable },
    { id: "runs", label: "Runs", icon: History },
  ];
  // The selected tab sits on --active (elev-7); hover on an unselected one is --hover
  // (elev-6). They must not be the same stop, or a hovered tab reads as the selected tab.
  function tabClass(t: RailTab): string {
    const base = "flex min-w-0 flex-1 items-center justify-center gap-1.5 rounded-lg px-1.5 py-1.5 transition-colors";
    return panelTab === t
      ? `${base} bg-active text-foreground`
      : `${base} text-muted-foreground hover:bg-hover hover:text-foreground`;
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
  let tuyaDevices = $state<{ device_id: string; name: string; online: boolean; functions_json?: string | null; function_labels_json?: string | null }[]>([]);
  let recipients = $state<{ label: string; enabled: boolean }[]>([]);
  let dictionaries = $state<{ id: string; name: string; columns: { name: string; type: string }[] }[]>([]);
  let rooms = $state<{ id: number; name: string }[]>([]);
  type Activity = {
    running: number;
    current: number | null;
    run_id?: string | null;
    trigger?: string | null;
    pod_name?: string | null;
  };
  let activity = $state<Record<string, Activity>>({});

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

  // Selecting a node IS the intent to inspect it, so jump to Properties — the same move
  // Unreal's Blueprint editor makes. Only on a fresh selection: deselecting leaves you
  // where you are, and a collapsed rail stays collapsed.
  let lastInspected: number | null = null;
  $effect(() => {
    const id = selectedNodeId;
    if (id != null && id !== lastInspected) panelTab = "properties";
    lastInspected = id;
  });

  const nodeById = $derived(new Map(graph?.nodes.map((n) => [n.id, n]) ?? []));
  // Nodes drawn with the standard chrome (header + ports). Decoration nodes (comment + the
  // display cards) render in their own layer with custom SVG bodies, so exclude them here.
  const nonCommentNodes = $derived(graph?.nodes.filter((n) => !isDecoration(n.kind)) ?? []);
  const displayNodes = $derived(graph?.nodes.filter((n) => isDisplayCard(n.kind)) ?? []);
  // Set of "<toNodeId>:<toPort>" for every wired input — lets isFilled()/requiredUnfilled()
  // be O(1) instead of scanning graph.edges per port per node on every edge change.
  const filledPorts = $derived(new Set((graph?.edges ?? []).map((e) => `${e.to}:${e.to_port}`)));
  const kinds = $derived(Object.values(catalog));

  // The bare get/set kinds are meaningless without a variable, so they never show in the
  // add-node menu directly — instead synthesize one "Get: <name>" / "Set: <name>" entry per
  // declared variable (typed `value` port + the variable baked in), mirroring per-event entries.
  function variableNodeEntries(): PaletteKind[] {
    const getBase = catalog["get_variable"];
    const setBase = catalog["set_variable"];
    if (!getBase || !setBase) return [];
    const out: PaletteKind[] = [];
    for (const v of variables) {
      const port: CatPort = { name: "value", type: v.type as PortType, required: false };
      const pv = { name: v.name, type: v.type };
      out.push({ ...getBase, label: `Get: ${v.name}`, presetVariable: pv, inputs: [], outputs: [port] });
      out.push({ ...setBase, label: `Set: ${v.name}`, presetVariable: pv, inputs: [{ ...port, required: true }], outputs: [] });
    }
    return out;
  }
  // What the add-node menu (ContextSearch) lists: all real kinds except the bare variable
  // placeholders (added via the Variables panel) + the reroute wire-helper (added by splitting a
  // wire), plus the per-variable Get/Set entries. `comment` IS listed — it has no other add path.
  const HIDE_FROM_MENU = new Set(["get_variable", "set_variable", "reroute"]);
  const menuKinds = $derived<PaletteKind[]>([
    ...(kinds as PaletteKind[]).filter((k) => !HIDE_FROM_MENU.has(k.slug)),
    ...variableNodeEntries(),
  ]);

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
  // Tuya smart-home device + data-point pickers (mirrors the camera picker: bind by the stable
  // device_id, label is the current name). The value widget for a `code` is typed from the
  // device's `functions_json`.
  type TuyaFn = { code: string; type: string; values?: string };
  const tuyaDeviceOptions = $derived(
    tuyaDevices.map((d) => ({ value: d.device_id, label: d.online ? d.name : `${d.name} (offline)` })),
  );
  function tuyaFunctions(deviceId: string): TuyaFn[] {
    const raw = tuyaDevices.find((d) => d.device_id === deviceId)?.functions_json;
    if (!raw) return [];
    try {
      const arr = JSON.parse(raw);
      return Array.isArray(arr) ? arr : [];
    } catch {
      return [];
    }
  }
  // Friendly labels the user set on the Integrations page ({ code: label }); fall back to the code.
  function tuyaLabels(deviceId: string): Record<string, string> {
    const raw = tuyaDevices.find((d) => d.device_id === deviceId)?.function_labels_json;
    if (!raw) return {};
    try {
      const o = JSON.parse(raw);
      return o && typeof o === "object" ? o : {};
    } catch {
      return {};
    }
  }
  function tuyaCodeOptions(deviceId: string): { value: string; label: string }[] {
    const labels = tuyaLabels(deviceId);
    return tuyaFunctions(deviceId).map((f) => ({ value: f.code, label: labels[f.code] || f.code }));
  }
  function tuyaFunction(deviceId: string, code: string): TuyaFn | undefined {
    return tuyaFunctions(deviceId).find((f) => f.code === code);
  }
  // The workflow PortType a Tuya data point maps to — baked into config.value_type so the node's
  // dynamic `value` input port resolves (Boolean→bool, Integer→num, Enum/String→text).
  function tuyaValueType(fn?: TuyaFn): "bool" | "num" | "text" {
    if (fn?.type === "Boolean") return "bool";
    if (fn?.type === "Integer") return "num";
    return "text";
  }
  function tuyaEnumRange(fn?: TuyaFn): string[] {
    if (!fn?.values) return [];
    try {
      const v = JSON.parse(fn.values);
      return Array.isArray(v.range) ? v.range : [];
    } catch {
      return [];
    }
  }
  const dictionaryOptions = $derived(dictionaries.map((d) => ({ value: d.id, label: d.name })));
  // Column picker for the Dictionary nodes (#113): once a node's `table` points at a dictionary, the
  // `column` field lists that dictionary's columns with their type, so you don't type names blind.
  const CELL_TYPE_LABEL: Record<string, string> = {
    text: "Text", num: "Number", bool: "Bool", camera: "Camera", recipient: "Recipient",
  };
  function columnOptions(tableId: string): { value: string; label: string }[] {
    const cols = dictionaries.find((d) => d.id === tableId)?.columns ?? [];
    return cols.map((c) => ({ value: c.name, label: `${c.name} · ${CELL_TYPE_LABEL[c.type] ?? c.type}` }));
  }

  // Dictionary create/edit modal (opened from a Table variable row). `dictModalVar` is the
  // variable index to point at the saved dictionary once a new one is created.
  let dictModalOpen = $state(false);
  let dictModalId = $state<string | null>(null);
  let dictModalVar = $state<number | null>(null);
  function openNewDictionary(i: number) {
    dictModalVar = i;
    dictModalId = null;
    dictModalOpen = true;
  }
  function openEditDictionary(i: number, id: string) {
    dictModalVar = i;
    dictModalId = id;
    dictModalOpen = true;
  }
  async function onDictionarySaved(saved: { id: string; name: string }) {
    await reloadDictionaries();
    // Point the originating variable at the new dictionary (only when creating a fresh one).
    if (dictModalVar !== null && dictModalId === null) {
      editVariable(dictModalVar, { default: saved.id });
    }
    dictModalVar = null;
  }
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
  // A node's `camera` config (id/ip/name/"first") resolved to the real device IP the
  // Region-drawing UI needs to fetch a snapshot for (the snapshot proxy only knows exact IPs).
  function resolvedCameraIp(stored: string): string {
    const key = cameraValue(stored);
    return devices.find((d) => cameraKey(d) === key)?.ip ?? "";
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

  // Same class of bug for the plain `camera` row: the dropdown DISPLAYS the first camera when the
  // stored value is empty (`cameraValue`'s fallback) but nothing persists it — and with a single
  // camera the Select can NEVER fire `onValueChange`, because picking the value it already shows
  // isn't a change. So the config stays "" while the panel claims a camera is selected. Downstream
  // that is silently fatal: control's `sync_tracking_config` drops an OnEvent tracking job whose
  // camera doesn't resolve, so tracking never starts and nothing tells the user why.
  // Fix: persist what the panel already shows, once, when such a node is selected. Skipped when the
  // camera arrives by a wire (the wire wins) or the graph is armed (no edits while running).
  $effect(() => {
    const node = selectedNode;
    if (!node || editLocked || cameraOptions.length === 0) return;
    const cfg = (node.config ?? {}) as Record<string, unknown>;
    const showsCameraRow =
      "camera" in cfg || !!effectiveKind(node)?.inputs.some((p) => p.name === "camera");
    if (!showsCameraRow) return;
    if (String(cfg.camera ?? "").trim()) return; // already stored — nothing to do
    if (wiredInto(node.id, "camera")) return; // resolved from upstream, not this literal
    const resolved = cameraValue("");
    if (resolved) saveNodeConfig(node, { ...node.config, camera: resolved });
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
  // `unfilled` is computed ONCE per row (in the wfRow snippet) and threaded into both, so a row
  // scans its nodes once instead of twice per render.
  function listRunDisabled(g: Graph, unfilled: boolean): boolean {
    return (
      runStatus(g) !== "disabled" ||
      triggerKind(g.trigger) !== "manual" ||
      !canRun(g) ||
      unfilled
    );
  }
  function listRunTitle(g: Graph, unfilled: boolean): string {
    const st = runStatus(g);
    if (st === "running") return "Running…";
    if (st === "idle") return "Active — stop it to run manually";
    if (triggerKind(g.trigger) !== "manual") return "Runs on a schedule — use the editor to run manually";
    if (unfilled) return "A node has a required input not filled";
    if (canRun(g)) return "Run now";
    return "Nothing to run — connect nodes or enable orphans";
  }
  // The emerald/amber/none marching-ants border on a list row.
  // The run state lives in the first-column dot now (it used to march a border around the row):
  // running = the live primary pulse, armed/idle = amber, disabled = calm grey.
  function dotState(g: Graph): "online" | "warn" | "offline" {
    const st = runStatus(g);
    if (st === "running") return "online";
    if (st === "idle") return "warn";
    return "offline";
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
    if (nodeLive(n.id) || isSelected(n.id)) return "stroke-primary";
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
    const MANAGED_KEYS: Record<string, string[]> = { string_switch: ["arms"], ai_switch: ["arms"], camera_switch: ["arms"], comment: ["width", "height"], dict_schema: ["columns"], dict_make_row: ["columns"], tuya_control: ["value_type"], tuya_read_status: ["value_type"], paragraph: ["width", "height"], markdown: ["width", "height"] };
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
    // StringSwitch / AiSwitch: exec outputs are the user-defined arm labels + built-in "default".
    if (node.kind === "string_switch" || node.kind === "ai_switch") {
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
    // Set keeps its exec pins (it's not pure) — append the value input to the existing ports.
    if (node.kind === "get_variable" || node.kind === "set_variable") {
      const ty = (typeof node.config?.type === "string" ? node.config.type : "text") as CatPort["type"];
      const port: CatPort = { name: "value", type: ty, required: node.kind === "set_variable" };
      return node.kind === "get_variable"
        ? { ...k, outputs: [port] }
        : { ...k, inputs: [...k.inputs, port] };
    }
    // Control Tuya Device: a `value` input typed by the selected data point (baked into
    // config.value_type when a code is picked) — wireable for a dynamic value; the config literal
    // (adaptive widget) is the unwired fallback. Mirrors Set Variable.
    if (node.kind === "tuya_control") {
      const ty = (typeof node.config?.value_type === "string" ? node.config.value_type : "bool") as CatPort["type"];
      return { ...k, inputs: [...k.inputs, { name: "value", type: ty, required: false }] };
    }
    // Read Tuya Status: the `value` output is typed by the selected data point (like the control
    // node's input), so a read wires straight into the right type; `on`/`raw` stay static.
    if (node.kind === "tuya_read_status") {
      const ty = (typeof node.config?.value_type === "string" ? node.config.value_type : "text") as CatPort["type"];
      return {
        ...k,
        outputs: [
          { name: "value", type: ty, required: false },
          { name: "on", type: "bool" as CatPort["type"], required: false },
          { name: "raw", type: "text" as CatPort["type"], required: false },
        ],
      };
    }
    // Create Dictionary Row (#113): the schema's columns (baked into config.columns when a
    // Dictionary Schema is wired) each become a typed input, so you fill the row field by field.
    if (node.kind === "dict_make_row") {
      const cols = Array.isArray(node.config?.columns) ? (node.config.columns as { name: string; type: string }[]) : [];
      const colPorts: CatPort[] = cols
        .filter((c) => c && String(c.name).trim())
        .map((c) => ({ name: String(c.name), type: (c.type ?? "text") as CatPort["type"], required: false }));
      return { ...k, inputs: [...k.inputs, ...colPorts] };
    }
    // AI Extract: `value` output typed per config.type (so it wires straight into Camera/Notify) + found.
    if (node.kind === "ai_extract") {
      const ty = (typeof node.config?.type === "string" ? node.config.type : "text") as CatPort["type"];
      return {
        ...k,
        outputs: [
          { name: "value", type: ty, required: false },
          { name: "found", type: "bool" as CatPort["type"], required: false },
        ],
      };
    }
    // OnEvent: camera + region GATING inputs (they filter delivery, not just display a value —
    // see docs/ref/workflows.md's "Gated events" section) for the edge tracker's three events
    // only. Extend this list + the backend's on_event_node match for the next event that needs
    // scoping — never a new node kind (see the same doc section).
    if (node.kind === "on_event") {
      const isTracking = TRACKING_EVENTS.includes(String(node.config?.event ?? ""));
      const gatingInputs: CatPort[] = isTracking
        ? [
            { name: "camera", type: "camera" as CatPort["type"], required: false },
            { name: "region", type: "region" as CatPort["type"], required: false },
          ]
        : [];
      // `class` + (for a region job) `zone_trigger` + the tracker-tuning knobs are plain config,
      // not ports (there's no "GetClassByName"/"GetTuning" node to wire — these are just fixed
      // values, low-risk to duplicate across a few nodes, unlike camera/region). Injected here
      // rather than in the backend's static default_config so they only appear once the node is
      // actually scoped to a tracking event, same spirit as camera/region. The tuning defaults
      // must match sentinel-edge's own (`default_min_hits`/etc., `sentinel-edge/src/tracking.rs`)
      // and sentinel-control's `sync_tracking_config` fallback constants — all three move together.
      const fields = payloadFields(node);
      const outPorts: CatPort[] = fields.map((f) => ({ name: f.name, type: f.type, required: false }));
      return {
        ...k,
        inputs: [...k.inputs, ...gatingInputs],
        outputs: [...k.outputs, ...outPorts],
        default_config: isTracking
          ? { ...k.default_config, class: "person", zone_trigger: "intersects", ...TRACKING_TUNING_DEFAULTS }
          : k.default_config,
      };
    }
    const fields = payloadFields(node);
    if (fields.length === 0) return k;
    const ports: CatPort[] = fields.map((f) => ({ name: f.name, type: f.type, required: false }));
    if (node.kind === "emit_event") return { ...k, inputs: [...k.inputs, ...ports] };
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
    if (n.kind === "string_switch" || n.kind === "camera_switch" || n.kind === "ai_switch") {
      const arms: unknown[] = Array.isArray(n.config?.arms) ? (n.config.arms as unknown[]) : [];
      return arms.length > 0 ? `${base} (${arms.length})` : base;
    }
    if (n.kind === "get_variable" || n.kind === "set_variable") {
      const v = String(n.config.variable ?? "").trim();
      const verb = n.kind === "get_variable" ? "Get" : "Set";
      return v ? `${verb} ${v}` : base;
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

  // ForEach lays out its outputs as: `loop_body` (top) → `item`/`index` (the per-iteration data) →
  // `completed` (bottom). `completed` fires once at the end, so it sits last, apart from the
  // loop-body pins. This is `completed`'s visual row (below loop_body + the data pins).
  const isForEach = (k: CatKind | undefined) => k?.slug === "for_each";
  const forEachCompletedRow = (k: CatKind | undefined) => 1 + dataPorts(k, "out").length;

  // Output data ports stack BELOW exec-out pins that live in the body.
  // Multi-exec nodes: ALL exec rows are in the body → offset = execCount.
  // Single-exec node: exec pin is in the header → no body rows consumed → offset = 0.
  // ForEach: only `loop_body` sits above the data pins (its `completed` moves to the bottom).
  function outDataOffset(k: CatKind | undefined): number {
    if (isForEach(k)) return 1;
    const count = execOutPorts(k).length;
    return count > 1 ? count : 0;
  }
  // Y where a node's body ends and the optional ShowImage preview begins.
  function portsBottom(n: Node): number {
    const k = effectiveKind(n);
    const execCount = execOutPorts(k).length;
    const outRows = dataPorts(k, "out").length + outDataOffset(k);
    // ForEach's `completed` pin sits at the bottom — make sure the node is tall enough for it.
    const forEachRows = isForEach(k) ? forEachCompletedRow(k) + 1 : 0;
    const rows = Math.max(dataPorts(k, "in").length, outRows, execCount, forEachRows, 1);
    return HEAD_H + nodeExprH(n) + rows * ROW_H;
  }
  // A ShowImage node grows to fit its preview thumbnail once a frame has arrived.
  function hasPreview(n: Node): boolean {
    return n.kind === "show_image" && !!nodeImages[n.id];
  }
  // Display live sinks (Text/Gauge) show a value readout below their ports (like ShowImage's preview).
  const TEXT_DISPLAY_H = 40;
  const GAUGE_DISPLAY_H = 92;
  function isValueSink(kind: string): boolean { return kind === "text" || kind === "gauge"; }
  function valueDisplayH(n: Node): number { return n.kind === "gauge" ? GAUGE_DISPLAY_H : TEXT_DISPLAY_H; }
  // The value a Text/Gauge node shows: the live runtime value, else its config literal.
  function sinkValue(n: Node): unknown { return n.id in nodeValues ? nodeValues[n.id] : n.config?.value; }
  function gaugeFraction(n: Node): number {
    const min = Number(n.config?.min ?? 0), max = Number(n.config?.max ?? 100);
    const v = Number(sinkValue(n) ?? min);
    if (!Number.isFinite(v) || max <= min) return 0;
    return Math.max(0, Math.min(1, (v - min) / (max - min)));
  }
  function nodeHeight(n: Node): number {
    const extra = hasPreview(n)
      ? PREVIEW_H + PAD
      : isValueSink(n.kind)
        ? valueDisplayH(n) + PAD
        : n.kind === "time_gate"
          ? 26
          : 0;
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
      if (hasMultipleExecOuts(k)) {
        const row = isForEach(k) && port === "completed" ? forEachCompletedRow(k) : Math.max(0, i);
        return HEAD_H + row * ROW_H + ROW_H / 2;
      }
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
  // Only comparison/logic operators use the compact "a OP b" middle layout. Conversion
  // and unpack nodes (num_to_text, break_camera, …) are plain data nodes — including them
  // here rendered a meaningless "?" operator in their middle.
  const PURE_KINDS = new Set([
    "num_lt","num_lte","num_gt","num_gte","num_eq","num_ne","compare","and","or","not","text_compare",
  ]);
  // Grow every node to fit its header title (icon + text + right padding) so long labels —
  // e.g. "When Event: telegram_message" or "Get current_recipient" — aren't clipped. Capped
  // to stay sane. Get/Set Variable keep a tighter 120px floor so short names (e.g. "Get x")
  // stay compact, but still widen to fit a long variable name.
  const NODE_W_MAX = 320;
  function nodeW(n: Node): number {
    const title = nodeTitle(n, catalog[n.kind]);
    const est = 46 + Math.ceil(title.length * 6.6); // 32 icon+pad + text + ~14 right pad
    const floor = n.kind === "get_variable" || n.kind === "set_variable" ? 120 : NODE_W;
    return Math.min(NODE_W_MAX, Math.max(floor, est));
  }
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
    return p ? portColor[p.type] : FALLBACK_PORT_COLOR;
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
  // Mirror the visual wire bends (#1) onto the local graph so a re-select restores them; they
  // persist with the rest of the canvas on Save (#122).
  function saveEdgeVias() {
    const id = selectedGraphId;
    if (id == null || editLocked) return;
    const payload: Record<string, [number, number][]> = {};
    for (const [k, pts] of Object.entries(edgeVias)) {
      payload[k] = pts.map((p) => [Math.round(p.x), Math.round(p.y)] as [number, number]);
    }
    const g = graphs.find((x) => x.id === id);
    if (g) g.edge_vias = payload;
    markDirty(); // wire bends are local; persist on Save (#122)
  }
  // The URL is the source of truth for which graph is open (/workflows/<id>).
  // openGraph / backToList only navigate; the $effect below applies the route to state,
  // so a refresh or a shared link lands straight in the right editor.
  async function openGraph(id: string) {
    if (dirty) await save(); // persist the current graph's canvas before switching (#122)
    void goto(`/workflows/${id}`, { keepFocus: true, noScroll: true });
  }
  async function backToList() {
    if (dirty) await save();
    void goto("/workflows", { keepFocus: true, noScroll: true });
  }
  // Apply an open-graph route to editor state (was the body of openGraph).
  function applyGraph(id: string) {
    saveViewport();
    selectedGraphId = id;
    selectedNodeId = null;
    pending = null;
    nodeImages = {}; // node ids restart per graph — drop the previous graph's previews
    nodeValues = {}; // …same for Text/Gauge value readouts
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
    // Pre-fetch persisted Text/Gauge values (node-state JSON `{value}`).
    const sinkNodes = graphs.find((x) => x.id === id)?.nodes.filter((n: Node) => isValueSink(n.kind)) ?? [];
    for (const n of sinkNodes) {
      fetch(data.nodeStateUrl(id, n.id)).then((r) => (r.ok ? r.json() : null)).then((st) => {
        if (st && "value" in st) nodeValues = { ...nodeValues, [n.id]: st.value };
      }).catch(() => {});
    }
    logs = [];
    void loadConsoleLogs(id);
    const v = graphs.find((x) => x.id === id)?.viewport;
    pan = v ? { x: v.x, y: v.y } : { x: 40, y: 40 };
    zoom = v?.zoom ?? 1;
    // Seed the persisted wire bends (#1) so the user's organisation is restored.
    const gv = graphs.find((x) => x.id === id)?.edge_vias;
    edgeVias = gv
      ? Object.fromEntries(Object.entries(gv).map(([k, pts]) => [k, pts.map(([x, y]) => ({ x, y }))]))
      : {};
    clearLive();
    view = "editor";
    void reconcileEventNodes(id); // self-heal event nodes against the current registry
    startTimeGatePolling();
  }
  // Apply the bare /workflows route to state (was the body of backToList).
  function showListView() {
    stopTimeGatePolling();
    saveViewport();
    selectedGraphId = null;
    selectedNodeId = null;
    pending = null;
    clearLive();
    view = "list";
    // Reconcile the list with any active filters (graphs may have been reloaded
    // unfiltered while editing).
    if (wfSearch.trim() || wfGroupFilter) void loadList();
  }
  // Route → state. Waits for graphs to load; an unknown id falls back to the list.
  $effect(() => {
    const raw = $page.params.id;
    const id = raw != null && raw !== "" ? raw : null;
    if (id != null) {
      if (graphs.length === 0) return; // graphs not loaded yet — re-runs when they are
      if (!graphs.some((g) => g.id === id)) {
        void goto("/workflows", { replaceState: true, keepFocus: true, noScroll: true });
        return;
      }
      if (selectedGraphId !== id || view !== "editor") applyGraph(id);
    } else if (view !== "list") {
      showListView();
    }
  });

  // ---- load ----------------------------------------------------------------
  // ---- list: search + group filter + last-run --------------------------------
  let wfSearch = $state("");
  let wfGroupFilter = $state(""); // "" = all groups
  let allGroups = $state<string[]>([]);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  // Which category roots are collapsed in the tree (by group key). Empty = all expanded.
  let collapsedGroups = $state<Set<string>>(new Set());
  function toggleGroup(key: string) {
    const next = new Set(collapsedGroups);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsedGroups = next;
  }

  async function reload() {
    // The editor needs the full set (find the open graph); load unfiltered here and
    // refresh the group options. The list view filters via loadList().
    graphs = await data.loadGraphs<Graph>();
    allGroups = [...new Set(graphs.map((g) => (g.group ?? "").trim()).filter(Boolean))].sort();
    if (selectedGraphId !== null && !graphs.some((g) => g.id === selectedGraphId)) {
      selectedGraphId = null;
    }
  }

  /** Backend-searched list (name + description + group), optionally scoped to one group. */
  async function loadList() {
    graphs = await data.loadGraphs<Graph>({ q: wfSearch.trim(), group: wfGroupFilter });
  }
  function onSearchInput(v: string) {
    wfSearch = v;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void loadList(), 250);
  }
  function setGroupFilter(v: string) {
    wfGroupFilter = v;
    void loadList();
  }
  function clearListFilters() {
    wfSearch = "";
    wfGroupFilter = "";
    void reload();
  }

  async function duplicateWorkflow(g: Graph) {
    try {
      const id = await data.duplicateGraph(g.id);
      await reload();
      openGraph(id);
    } catch (err) {
      toast.error("Couldn't duplicate workflow", { description: String(err) });
    }
  }

  // ---- templates (#102) ----------------------------------------------------
  const isMaster = $derived(auth.me?.is_master ?? false);

  // List view: two tabs — the tenant's own workflows, and the template catalog (own + global)
  // shown inline so a brand-new user meets the starter templates immediately (#136).
  type TemplateSummary = { id: string; name: string; description: string; global: boolean; inputs: { name: string; type: string }[] };
  let listTab = $state<"mine" | "templates" | "dictionaries">("mine");
  let listTabInit = false; // pick the initial tab once, after the first load
  let templates = $state<TemplateSummary[]>([]);
  let templatesLoading = $state(false);
  let usingTemplateId = $state<string | null>(null);
  function showTemplatesTab() {
    listTab = "templates";
    if (!templates.length && !templatesLoading) void loadTemplates();
  }
  async function loadTemplates() {
    templatesLoading = true;
    try {
      templates = await data.listTemplates<TemplateSummary>();
    } catch (err) {
      toast.error("Couldn't load templates", { description: String(err) });
    } finally {
      templatesLoading = false;
    }
  }
  // Clone a template into this tenant, then open the new (disabled) workflow to fill its inputs.
  async function cloneTemplate(t: TemplateSummary) {
    usingTemplateId = t.id;
    try {
      const id = await data.useTemplate(t.id);
      toast.info(`Created “${t.name}” — fill in its inputs and turn it on.`);
      goto(`/workflows/${id}`);
    } catch (err) {
      usingTemplateId = null;
      toast.error("Couldn't use template", { description: String(err) });
    }
  }

  // Publish/unpublish a template to the GLOBAL catalog — master only (the endpoint 403s others).
  async function toggleGlobal(g: Graph) {
    const next = !g.global;
    g.global = next;
    if (next) g.template = true;
    try {
      await data.setGraphGlobal(g.id, next);
    } catch (err) {
      g.global = !next;
      toast.error("Couldn't publish template", { description: String(err) });
    }
  }

  // Mark/unmark a graph as one of the tenant's own templates — optimistic, like the other list ops.
  async function toggleTemplate(g: Graph) {
    const next = !g.template;
    g.template = next;
    try {
      await data.setGraphTemplate(g.id, next);
    } catch (err) {
      g.template = !next;
      toast.error("Couldn't update template", { description: String(err) });
    }
  }

  async function loadRefs() {
    try {
      const [d, r, m, dicts, tuya, phones] = await Promise.all([
        data.listDevices(),
        data.listBeacons(),
        data.loadMap(),
        data.listDictionaries().catch(() => []),
        data.listTuyaDevices().catch(() => []),
        data.listPushDevices().catch(() => []),
      ]);
      devices = d;
      // A registered phone is its own Recipient (card #139) — merge push devices into the picker
      // alongside the Telegram beacons so a workflow can address one phone. Labels are the
      // addressing key on both sides; drop a phone whose label collides with a beacon.
      const phoneRows = phones
        .filter((p) => p.label && !r.some((b) => b.label === p.label))
        .map((p) => ({ label: p.label, enabled: p.enabled }));
      recipients = [...r, ...phoneRows];
      dictionaries = dicts.map((x) => ({ id: x.id, name: x.name, columns: x.columns ?? [] }));
      rooms = (m.rooms ?? []).map((rm) => ({ id: rm.id, name: rm.name }));
      tuyaDevices = tuya.map((t) => ({ device_id: t.device_id, name: t.name, online: t.online, functions_json: t.functions_json, function_labels_json: t.function_labels_json }));
    } catch (err) {
      console.error("Failed to load devices/recipients/rooms", err);
    }
  }
  async function reloadDictionaries() {
    try {
      dictionaries = (await data.listDictionaries()).map((x) => ({ id: x.id, name: x.name, columns: x.columns ?? [] }));
    } catch (err) {
      console.error("Failed to reload dictionaries", err);
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
      activity = await data.runStates<Record<string, Activity>>();
    } catch {
      /* ignore transient errors */
    }
  }

  onMount(() => {
    isTouch = window.matchMedia?.("(pointer: coarse)").matches ?? false;
    const savedW = Number(localStorage.getItem("wf.panelW"));
    if (savedW >= PANEL_MIN && savedW <= PANEL_MAX) panelW = savedW;
    const savedOpen = localStorage.getItem("wf.panelOpen");
    if (savedOpen != null) panelOpen = savedOpen === "true";
    const savedTab = localStorage.getItem("wf.panelTab");
    if (savedTab === "details" || savedTab === "properties" || savedTab === "variables" || savedTab === "runs") {
      panelTab = savedTab;
    }
    const savedConsoleOpen = localStorage.getItem("wf.consoleOpen");
    if (savedConsoleOpen != null) consoleOpen = savedConsoleOpen === "true";
    const savedConsoleH = Number(localStorage.getItem("wf.consoleH"));
    if (savedConsoleH >= CONSOLE_MIN && savedConsoleH <= CONSOLE_MAX) consoleH = savedConsoleH;
    (async () => {
      try {
        // Fire everything CONCURRENTLY. This was six serial awaits, so the view waited for the
        // SUM of the round-trips (catalog → graphs → devices/beacons/map → events → runs). Now the
        // slowest single call gates the view. `nodeCatalog` folds into `catalog` as it resolves.
        await Promise.all([
          data.nodeCatalog<{ kinds: CatKind[] }>().then((cat) => {
            catalog = Object.fromEntries(cat.kinds.map((k) => [k.slug, k]));
          }),
          reload(),
          loadRefs(),
          loadEvents(),
          refreshActivity(),
        ]);
        // A brand-new user (no workflows yet) lands on the Templates tab so the starter catalog
        // is the first thing they see, not an empty list (#136). Decide once, post-load.
        if (!listTabInit) {
          listTabInit = true;
          if (graphs.length === 0) listTab = "templates";
        }
        void loadTemplates();
      } catch (err) {
        toast.error("Failed to load workflows", { description: String(err) });
      } finally {
        loading = false;
      }
    })();
    const un = onEvent("workflow-run", (payload) => {
      running = false;
      const { graph: gid, name, ok, summary } = payload as {
        graph: string;
        name: string;
        ok: boolean;
        summary: string;
      };
      // Run finished (ok or error) → drain remaining highlights, then stop. Outcomes surface in
      // the Console (open graph), the live node highlighting and the activity map — no per-run
      // toast: it fired for every run of every graph in the tenant, which was pure noise (#103).
      if (gid === selectedGraphId) {
        endLive();
        logs = [{ time: fmtTs(new Date().toISOString()), name, ok, summary }, ...logs].slice(0, 200);
      }
    });
    // Per-node execution events (real-time, catch every node).
    const unNode = onEvent("workflow-node", (payload) => {
      const p = payload as { graph: string; node: number };
      if (p.graph === selectedGraphId) markLive(p.node);
    });
    // Per-node debug logs (only emitted when the graph's debug flag is on).
    const unLog = onEvent("workflow-log", (payload) => {
      const p = payload as { graph: string; node: number; label: string; message: string; ms: number };
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
      const p = payload as { graph?: string; message: string; level: string; on_screen: boolean };
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
        graph: string;
        node: number;
        label: string;
      };
      if (gid !== selectedGraphId) return; // node ids are per-graph — only the open one
      nodeImages = { ...nodeImages, [node]: { src: data.runFrameUrl(gid, node, Date.now()), label } };
    });
    // Show value node (Text/Gauge): the small value rides SSE inline — render it live in the node.
    const unValue = onEvent("workflow-value", (payload) => {
      const p = payload as { graph: string; node: number; value: unknown };
      if (p.graph !== selectedGraphId) return; // node ids are per-graph — only the open one
      nodeValues = { ...nodeValues, [p.node]: p.value };
    });
    // Cross-graph events: dispatched on the emitter, triggered/skipped on each listener.
    // Show the line in whichever graph's run view is open (matched by `graph`).
    const unEvent = onEvent("workflow-event", (payload) => {
      const p = payload as {
        graph: string | null;
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
        graph: string;
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
    let streamOpened = false;
    const unOpen = onEvent("stream-open", () => {
      // Skip the FIRST open — mount already fetched activity, so refreshing here just duplicates
      // GET /workflows/runs at startup. Refresh only on a RE-open (reconnect), to re-sync after a
      // dropped stream.
      if (!streamOpened) {
        streamOpened = true;
        return;
      }
      void refreshActivity();
    });
    return () => {
      saveViewport(); // persist the viewport when leaving the page too
      un.then((u) => u());
      unNode.then((u) => u());
      unLog.then((u) => u());
      unPrint.then((u) => u());
      unImage.then((u) => u());
      unValue.then((u) => u());
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
  // Metadata (name/description/group/enabled/…) has its OWN controls + persists immediately. Update
  // local state optimistically and DON'T reload — a reload would re-fetch the server graph and wipe
  // unsaved canvas edits (#122).
  async function renameInline(name: string) {
    if (!graph) return;
    const n = name.trim();
    if (!n || n === graph.name) return;
    graph.name = n;
    markDirty(); // already auto-saved below; also light the Save indicator so it reflects every edit
    try {
      await data.renameGraph(graph.id, n);
    } catch (err) {
      toast.error("Rename failed", { description: String(err) });
    }
  }
  async function saveDescription(v: string) {
    if (!graph || (graph.description ?? "") === v) return;
    graph.description = v;
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphDescription(graph.id, v);
    } catch (err) {
      toast.error("Couldn't save description", { description: String(err) });
    }
  }
  async function saveGroup(v: string) {
    if (!graph) return;
    const val = v.trim();
    if ((graph.group ?? "") === val) return;
    graph.group = val;
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphGroup(graph.id, val);
    } catch (err) {
      toast.error("Couldn't save group", { description: String(err) });
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
    g.enabled = enabled; // optimistic — reflect the flip instantly; no reload (would wipe canvas)
    try {
      await data.setGraphEnabled(g.id, enabled);
    } catch (err) {
      g.enabled = !enabled; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function toggleRunOrphans(g: Graph, run_orphans: boolean) {
    g.run_orphans = run_orphans; // optimistic
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphRunOrphans(g.id, run_orphans);
    } catch (err) {
      g.run_orphans = !run_orphans; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  async function toggleDebug(g: Graph, debug: boolean) {
    g.debug = debug; // optimistic
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphDebug(g.id, debug);
    } catch (err) {
      g.debug = !debug; // roll back on failure
      toast.error("Couldn't update", { description: String(err) });
    }
  }
  // Tooltips for the top-of-canvas state toggles (Enabled / Debug / Orphans).
  function enabledTitle(): string {
    return graph?.enabled
      ? "Armed — the workflow's trigger is live. Click to disarm."
      : "Disarmed — click to arm this workflow's trigger.";
  }
  function debugTitle(): string {
    if (editLocked) return "Debug — locked while the workflow is armed or running.";
    return graph?.debug
      ? "Debug on — logging each node's result to the console. Click to stop."
      : "Debug off — log each node's result to the console during a run.";
  }
  function orphansTitle(): string {
    if (editLocked) return "Run orphans — locked while the workflow is armed or running.";
    return graph?.run_orphans
      ? "Run orphans on — unconnected nodes run. Click to skip them."
      : "Run orphans off — nodes with no connections are skipped during a run.";
  }

  // A manual run with the given input values (empty = use defaults).
  async function doRun(g: Graph, inputs?: Record<string, string>) {
    running = true;
    try {
      const msg = await data.runGraph(g.id, inputs);
      toast.info(msg);
    } catch (err) {
      running = false;
      toast.error("Run", { description: String(err) });
    }
  }

  // Run prompt (#102): when a graph declares input variables (`param`), ask for their values before
  // running — pre-filled with each input's current default. A graph with no inputs runs immediately.
  let runPromptOpen = $state(false);
  let runPromptGraph = $state<Graph | null>(null);
  let runInputs = $state<Record<string, string>>({});
  const runParams = $derived<VarDecl[]>((runPromptGraph?.variables ?? []).filter((v) => v.param));

  function runNow(g: Graph) {
    const params = (g.variables ?? []).filter((v) => v.param);
    if (params.length === 0) {
      doRun(g);
      return;
    }
    runInputs = Object.fromEntries(params.map((v) => [v.name, v.default ?? ""]));
    runPromptGraph = g;
    runPromptOpen = true;
  }

  function confirmRun() {
    const g = runPromptGraph;
    runPromptOpen = false;
    if (g) doRun(g, { ...runInputs });
  }

  // ---- trigger / concurrency ----------------------------------------------
  function triggerKind(t: Trigger): "manual" | "cron" | "schedule" {
    if (t === "Manual") return "manual";
    if (typeof t === "object" && "Cron" in t) return "cron";
    return "schedule";
  }
  function triggerSecs(t: Trigger): number {
    return typeof t === "object" && "Cron" in t ? t.Cron.every_secs : 60;
  }
  // ---- Calendar schedule (#111) ----
  const WEEKDAY_LABELS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const MONTH_LABELS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const SCHED_FREQ_OPTS: { value: ScheduleFreq; label: string }[] = [
    { value: "hourly", label: "Every hour" },
    { value: "daily", label: "Daily" },
    { value: "weekly", label: "Weekly" },
    { value: "monthly", label: "Monthly" },
    { value: "yearly", label: "Yearly" },
  ];
  const BROWSER_TZ = typeof Intl !== "undefined" ? Intl.DateTimeFormat().resolvedOptions().timeZone : "UTC";
  function defaultSchedule(): ScheduleT {
    return { freq: "daily", tz: BROWSER_TZ, times: ["09:00"], minute: 0, weekdays: [0, 1, 2, 3, 4], day_of_month: 1, month: 1 };
  }
  function schedOf(t: Trigger): ScheduleT {
    return typeof t === "object" && "Schedule" in t ? { ...defaultSchedule(), ...t.Schedule } : defaultSchedule();
  }
  function ordinal(n: number): string {
    const s = ["th", "st", "nd", "rd"], v = n % 100;
    return n + (s[(v - 20) % 10] || s[v] || s[0]);
  }
  function scheduleSummary(s: ScheduleT): string {
    const times = s.times.length ? s.times.join(", ") : "—";
    if (s.freq === "hourly") return `Hourly at :${String(s.minute).padStart(2, "0")}`;
    if (s.freq === "weekly") {
      const days = [...s.weekdays].sort((a, b) => a - b).map((d) => WEEKDAY_LABELS[d]).join(", ") || "—";
      return `${days} at ${times}`;
    }
    if (s.freq === "monthly") return `Monthly on the ${ordinal(s.day_of_month)} at ${times}`;
    if (s.freq === "yearly") return `${MONTH_LABELS[(s.month || 1) - 1]} ${s.day_of_month} at ${times}`;
    return `Daily at ${times}`;
  }
  function triggerSummary(t: Trigger): string {
    if (t === "Manual") return "Manual";
    if (typeof t === "object" && "Cron" in t) return `Every ${t.Cron.every_secs}s`;
    return scheduleSummary(schedOf(t));
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
    // Trigger persists immediately (its own endpoint); reflect locally with no reload —
    // a reload would wipe unsaved canvas edits (#122).
    g.trigger = kind === "cron" ? { Cron: { every_secs: everySecs } } : "Manual";
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphTrigger(g.id, kind, everySecs);
    } catch (err) {
      toast.error("Couldn't set trigger", { description: String(err) });
    }
  }
  // Switch trigger kind from the dropdown — schedule seeds/keeps its own config.
  function onTriggerKind(g: Graph, kind: string) {
    if (kind === "schedule") applySchedule(g, schedOf(g.trigger));
    else applyTrigger(g, kind, triggerSecs(g.trigger));
  }
  async function applySchedule(g: Graph, sched: ScheduleT) {
    g.trigger = { Schedule: sched } as Trigger;
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphTrigger(g.id, "schedule", undefined, sched);
    } catch (err) {
      toast.error("Couldn't set schedule", { description: String(err) });
    }
  }
  // Merge one field change into the current schedule + persist.
  function patchSchedule(g: Graph, patch: Partial<ScheduleT>) {
    applySchedule(g, { ...schedOf(g.trigger), ...patch });
  }
  function toggleWeekday(g: Graph, day: number) {
    const cur = schedOf(g.trigger).weekdays;
    const next = cur.includes(day) ? cur.filter((d) => d !== day) : [...cur, day];
    patchSchedule(g, { weekdays: next });
  }
  function setScheduleTime(g: Graph, i: number, value: string) {
    const times = [...schedOf(g.trigger).times];
    times[i] = value;
    patchSchedule(g, { times });
  }
  function addScheduleTime(g: Graph) {
    patchSchedule(g, { times: [...schedOf(g.trigger).times, "12:00"] });
  }
  function removeScheduleTime(g: Graph, i: number) {
    const times = schedOf(g.trigger).times.filter((_, idx) => idx !== i);
    patchSchedule(g, { times: times.length ? times : ["09:00"] });
  }
  async function applyConcurrency(g: Graph, kind: string, max: number) {
    g.concurrency =
      kind === "queue" ? { QueueSingle: { max_queued: max } } : kind === "concurrent" ? { Concurrent: { max_concurrent: max } } : "SkipIfRunning";
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphConcurrency(g.id, kind, max);
    } catch (err) {
      toast.error("Couldn't set concurrency", { description: String(err) });
    }
  }
  async function applyInstanceScope(g: Graph, kind: string) {
    g.instance_scope = kind === "per_camera" ? "PerCamera" : "Single";
    markDirty(); // surface the edit in the Save indicator too (this setting also auto-saves)
    try {
      await data.setGraphInstanceScope(g.id, kind);
    } catch (err) {
      toast.error("Couldn't set instance scope", { description: String(err) });
    }
  }

  // ---- node + edge ops -----------------------------------------------------
  async function addNodeKind(slug: string, presetEvent?: string, presetVariable?: { name: string; type: string }): Promise<number | null> {
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
    // Default camera/recipient configs to the actual first item in each list; for a
    // synthesized event entry, pre-bind its event name.
    const cfg = { ...(catalog[slug]?.default_config ?? {}) };
    if ("camera" in cfg && devices[0]) cfg.camera = cameraKey(devices[0]); // bind by stable id (not name)
    if ("tuya_device" in cfg && tuyaDevices[0]) cfg.tuya_device = tuyaDevices[0].device_id; // bind by stable id
    if ("recipient" in cfg && recipients[0]) cfg.recipient = recipients[0].label;
    if (presetEvent) {
      cfg.event = presetEvent;
      cfg.payload = eventFields(presetEvent); // bake payload ports onto the new event node
    }
    if (presetVariable) {
      cfg.variable = presetVariable.name;
      cfg.type = presetVariable.type; // bake the variable's name + type → its dynamic port
    }
    // Local + instant: a real client-assigned id, no server round-trip. Persists on Save (#122).
    // memoize defaults to FALSE — matching Rust `Graph::add_node`. Sending it explicitly is
    // required: omitting it makes the backend's `#[serde(default = "default_true")]` flip it ON,
    // which silently breaks loop bodies (the node caches after one pass and won't iterate).
    const g = graph;
    const id = nextNodeId(g);
    g.nodes.push({ id, kind: slug, x, y, config: cfg, memoize: false } as Node);
    selectedNodeId = id;
    markDirty();
    return id;
  }

  // ---- workflow variables (the Variables tab in the right rail) --------------
  // Renders straight off the local graph, so mutating `graph.variables` in place updates the panel
  // INSTANTLY — no await, no reload. Persistence is a debounced background PUT (mirrors the
  // optimistic node ops + liveSaveConfig). Before this, every keystroke awaited a PUT *then* a full
  // `GET /workflows` reload, which was the lag.
  const variables = $derived<VarDecl[]>(graph?.variables ?? []);

  let varsSaveTimer: ReturnType<typeof setTimeout> | undefined;
  // Persist the given graph's current variables in the background, debounced so typing a name or
  // default doesn't PUT per keystroke. Reads the live array at fire time so the last edit wins.
  function scheduleVarSave(gid: string) {
    clearTimeout(varsSaveTimer);
    varsSaveTimer = setTimeout(() => {
      const g = graphs.find((x) => x.id === gid);
      if (!g) return;
      data
        .setGraphVariables(
          gid,
          (g.variables ?? []).map((v) => ({
            name: v.name,
            type: v.type,
            default: v.default ?? null,
            param: v.param ?? false,
            description: v.description ?? null,
          })),
        )
        .catch((err) => console.error("Failed to save variables", err));
    }, 400);
  }
  function uniqueVarName(existing: VarDecl[]): string {
    const taken = new Set(existing.map((v) => v.name));
    let i = existing.length + 1;
    while (taken.has(`var${i}`)) i++;
    return `var${i}`;
  }
  function addVariable() {
    if (editLocked || !graph) return;
    graph.variables = [...(graph.variables ?? []), { name: uniqueVarName(variables), type: "text" }];
    markDirty(); // surface the change in the Save indicator (vars auto-save via scheduleVarSave)
    scheduleVarSave(graph.id);
  }
  // Edit a variable + re-sync the baked name/type on any Get/Set nodes that reference it, so
  // existing nodes keep a valid port type after a rename/retype. All local + background — instant.
  function editVariable(i: number, patch: Partial<VarDecl>) {
    if (editLocked || !graph) return;
    const old = graph.variables?.[i];
    if (!old) return;
    // Retyping invalidates a default typed for the old type — clear it.
    if (patch.type && patch.type !== old.type && patch.default === undefined) patch = { ...patch, default: null };
    if (patch.name === "" || (patch.name && variables.some((v, j) => j !== i && v.name === patch.name))) return; // non-empty + unique
    const updated = { ...old, ...patch };
    graph.variables = graph.variables!.map((v, j) => (j === i ? updated : v));
    if (patch.name || patch.type) {
      for (const n of graph.nodes) {
        if ((n.kind === "get_variable" || n.kind === "set_variable") && n.config?.variable === old.name) {
          n.config = { ...n.config, variable: updated.name, type: updated.type }; // local; persisted on Save (#122)
        }
      }
    }
    markDirty(); // any variable edit (incl. default-only) should light the Save indicator; vars auto-save via scheduleVarSave
    scheduleVarSave(graph.id);
  }
  function removeVariable(i: number) {
    if (editLocked || !graph) return;
    graph.variables = (graph.variables ?? []).filter((_, j) => j !== i);
    markDirty(); // surface the change in the Save indicator (vars auto-save via scheduleVarSave)
    scheduleVarSave(graph.id);
  }
  // ---- drag a variable from the panel onto the canvas (Unreal-style) -------
  const VAR_DRAG_MIME = "application/x-sentinel-variable";
  // After a drop, a small chooser asks Get vs Set at the drop point.
  let varDrop = $state<{ world: { x: number; y: number }; screen: { x: number; y: number }; v: { name: string; type: string } } | null>(null);

  function onCanvasDragOver(e: DragEvent) {
    if (!editLocked && e.dataTransfer?.types.includes(VAR_DRAG_MIME)) {
      e.preventDefault(); // allow the drop
      e.dataTransfer.dropEffect = "copy";
    }
  }
  function onCanvasDrop(e: DragEvent) {
    const raw = e.dataTransfer?.getData(VAR_DRAG_MIME);
    if (!raw || editLocked || !svgEl) return;
    e.preventDefault();
    let v: { name: string; type: string };
    try {
      v = JSON.parse(raw);
    } catch {
      return;
    }
    const r = svgEl.getBoundingClientRect();
    const world = { x: (e.clientX - r.left - pan.x) / zoom, y: (e.clientY - r.top - pan.y) / zoom };
    varDrop = { world, screen: { x: e.clientX, y: e.clientY }, v };
  }
  async function chooseVarNode(kind: "get_variable" | "set_variable") {
    const d = varDrop;
    varDrop = null;
    if (!d) return;
    dropAt = d.world; // addNodeKind places at this world point
    await addNodeKind(kind, undefined, d.v);
  }

  // Add a node from context search and auto-wire it to the pending output port.
  async function addFromContext(slug: string, presetEvent?: string, presetVariable?: { name: string; type: string }) {
    const cp = contextPending;
    contextOpen = false;
    contextPending = null;
    const id = await addNodeKind(slug, presetEvent, presetVariable);
    if (cp && id !== null && graph) {
      // A Set Variable node's input is its dynamic typed `value` port, not a static catalog port.
      const candidateInputs: CatPort[] =
        presetVariable && slug === "set_variable"
          ? [{ name: "value", type: presetVariable.type as PortType, required: true }]
          : (catalog[slug]?.inputs ?? []);
      const matchPort =
        candidateInputs.find((p) => p.type === cp.portType) ??
        candidateInputs.find((p) => p.type === "any");
      if (matchPort) {
        graph.edges.push({ from: cp.node, from_port: cp.port, to: id, to_port: matchPort.name });
        markDirty();
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

    // Backtick (`): collapse/expand the chrome (rail + console) as a group. Rebound off Tab
    // (#105) so Tab is left entirely to the browser's focus navigation — the `inField` guard
    // above already keeps this from firing mid-typing, so no focus-based guard is needed.
    if (e.key === "`") {
      e.preventDefault();
      toggleChrome();
      return;
    }

    // Tool shortcuts (mouse parity for the touch tool palette): v move, h pan, l link, b break.
    const toolKey: Record<string, typeof editorMode> = { v: "move", h: "pan", l: "link", b: "break" };
    if (toolKey[e.key] && !contextOpen) {
      e.preventDefault();
      editorMode = toolKey[e.key];
      pending = null; // switching tools cancels an in-progress wire
      return;
    }

    // n / N: open node search palette at cursor position.
    if ((e.key === "n" || e.key === "N") && !editLocked && !contextOpen) {
      e.preventDefault();
      dropAt = hoverWorld;
      contextPos = hoverClient;
      contextPending = null;
      contextOpen = true;
      return;
    }

    // Delete / Backspace: delete every selected node (the whole marquee/shift set).
    if ((e.key === "Delete" || e.key === "Backspace") && !editLocked) {
      const ids = selectedIds.size ? [...selectedIds] : selectedNodeId !== null ? [selectedNodeId] : [];
      const targets = (graph?.nodes ?? []).filter((n) => ids.includes(n.id));
      if (targets.length) { e.preventDefault(); for (const t of targets) removeNode(t); }
      return;
    }

    // Save the canvas (⌘S / Ctrl-S) — works even when armed (nothing to save then, but harmless).
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (dirty) void save();
      return;
    }

    // Copy / Cut / Paste / Duplicate of the selection (Cmd/Ctrl).
    if ((e.metaKey || e.ctrlKey) && !editLocked) {
      const k = e.key.toLowerCase();
      if (k === "c") { copySelection(); e.preventDefault(); return; }
      if (k === "x") { copySelection(); deleteSelection(); e.preventDefault(); return; }
      if (k === "v") { pasteClipboard(); e.preventDefault(); return; }
      if (k === "d") { copySelection(); pasteClipboard(); e.preventDefault(); return; }
      if (k === "a") { selectAll(); e.preventDefault(); return; }
    }
  }

  // Unreal-Blueprint-style header tint by category, so node families read at a glance and
  // events POP. Returns a hex colour; the header band is drawn at low alpha behind the label.
  function headerColor(kind: string): string {
    if (kind === "emit_event" || kind === "on_event") return EVENT_HEADER_COLOR;
    return CATEGORY_COLOR[catalog[kind]?.category ?? ""] ?? DEFAULT_CATEGORY_COLOR;
  }
  // Category → glyph. Keys match the Rust catalog's category() strings; mirrors the
  // header tint so a node's colour and icon agree.
  const CATEGORY_ICON: Record<string, typeof Box> = {
    Cameras: Camera,
    Vision: Eye,
    AI: Sparkles,
    Alerts: Bell,
    Recipients: Send,
    Text: Type,
    Network: Globe,
    Control: Waypoints,
    Logic: Binary,
    Presence: Radar,
    Recording: Disc,
    Debug: Bug,
    Utility: Waypoints,
    Variables: Variable,
  };
  function categoryIcon(n: Node): typeof Box {
    // Recording gets its own record/stop identity, distinct from the camera family.
    if (n.kind === "start_recording") return Disc;
    if (n.kind === "stop_recording") return CircleStop;
    if (n.kind === "emit_event" || n.kind === "on_event") return Zap;
    if (n.kind === "get_variable" || n.kind === "set_variable") return Variable;
    // AI Switch lives in the Control palette (it's a router) but is AI-powered — give it the AI glyph.
    if (n.kind === "ai_switch") return Sparkles;
    return CATEGORY_ICON[catalog[n.kind]?.category ?? ""] ?? Box;
  }
  // Node-aware header colour: Get/Set Variable nodes take their variable's TYPE colour, so a
  // variable's get + set read as one family on the canvas. Everything else is by category.
  function headerColorFor(n: Node): string {
    if (n.kind === "get_variable" || n.kind === "set_variable") {
      const t = typeof n.config?.type === "string" ? n.config.type : "text";
      return portColor[t as PortType] ?? headerColor(n.kind);
    }
    if (n.kind === "ai_switch") return CATEGORY_COLOR.AI; // AI-powered router → AI colour
    return headerColor(n.kind);
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
  async function reconcileEventNodes(graphId: string) {
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
      for (const idx of removeIdx.sort((a, b) => b - a)) g.edges.splice(idx, 1);
      for (const u of updates) {
        const nn = g.nodes.find((x) => x.id === u.id);
        if (nn) nn.config = u.config;
      }
      markDirty();
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
    const cfg: Record<string, unknown> = { ...node.config, event: ev, payload: eventFields(ev) };
    // Bake the property panel's own displayed fallbacks (`class` "person", `zone_trigger`
    // "intersects" — see `effectiveKind`'s on_event branch) into the SAVED config the moment a
    // node becomes scoped to a tracking event, so a node the user never touches beyond picking
    // the event still resolves to a real job server-side (`sync_tracking_config` reads only
    // what's actually persisted, not the UI's display-time fallback).
    if (TRACKING_EVENTS.includes(ev)) {
      if (cfg.class === undefined) cfg.class = "person";
      if (cfg.zone_trigger === undefined) cfg.zone_trigger = "intersects";
      for (const [key, def] of Object.entries(TRACKING_TUNING_DEFAULTS)) {
        if (cfg[key] === undefined) cfg[key] = def;
      }
    }
    saveNodeConfig(node, cfg);
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

  // ---- recent runs ----------------------------------------------------------
  // Two entry points share one loader: the Runs TAB in the editor's rail (no graph arg —
  // it's the graph you're editing), and the modal drawer on the workflow LIST view, which
  // has no rail to put a tab in.
  let runsLoading = $state(false);
  async function loadRuns(g: Graph | null = graph) {
    if (!g) return;
    runsGraph = g;
    runsList = [];
    runsLogs = {};
    runsExpanded = null;
    runsLoading = true;
    try {
      runsList = await data.runHistory<RunRecord[]>(g.id, 50);
    } catch (err) {
      toast.error("Couldn't load run history", { description: String(err) });
    } finally {
      runsLoading = false;
    }
  }
  async function openRuns(g: Graph) {
    runsOpen = true;
    await loadRuns(g);
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
  function comingSoon(what: string) {
    toast.info(`${what} — coming soon`);
  }
  async function removeNode(n: Node) {
    if (!graph || editLocked) return;
    const g = graph;
    const gid = graph.id;
    // Optimistic: drop the node + its wires locally NOW, persist in the background, no reload.
    // Capture what we removed so we can put it back if the server rejects the delete.
    const nodeIdx = g.nodes.findIndex((x) => x.id === n.id);
    if (nodeIdx < 0) return;
    const removedNode = g.nodes[nodeIdx];
    const removedEdges = g.edges.filter((e) => e.from === n.id || e.to === n.id);
    g.nodes.splice(nodeIdx, 1);
    g.edges = g.edges.filter((e) => e.from !== n.id && e.to !== n.id);
    if (selectedNodeId === n.id) selectedNodeId = null;
    if (selectedIds.has(n.id)) {
      const s = new Set(selectedIds);
      s.delete(n.id);
      selectedIds = s;
    }
    if (nodeImages[n.id]) {
      const { [n.id]: _drop, ...rest } = nodeImages;
      nodeImages = rest;
    }
    markDirty(); // node + its edges already removed from local state; persists on Save (#122)
  }
  // ---- multi-node clipboard: copy / paste / duplicate the selection ---------
  function selectedNodeList(): Node[] {
    const ids = selectedIds.size
      ? selectedIds
      : selectedNodeId !== null
        ? new Set([selectedNodeId])
        : new Set<number>();
    return (graph?.nodes ?? []).filter((n) => ids.has(n.id));
  }
  function selectAll() {
    if (!graph) return;
    selectedIds = new Set(graph.nodes.map((n) => n.id));
    selectedNodeId = graph.nodes.at(-1)?.id ?? null;
  }
  function copySelection() {
    const nodes = selectedNodeList();
    const ids = new Set(nodes.map((n) => n.id));
    const cloned = nodes.map(
      (n) => ({ id: n.id, kind: n.kind, x: n.x, y: n.y, config: $state.snapshot(n.config), memoize: n.memoize }) as Node,
    );
    // Only wires whose BOTH ends are in the copied set travel with it (internal wiring).
    const edges = (graph?.edges ?? []).filter((e) => ids.has(e.from) && ids.has(e.to)).map((e) => ({ ...e }));
    clipboard = { nodes: cloned, edges };
  }
  function deleteSelection() {
    for (const n of selectedNodeList()) removeNode(n);
  }
  // Paste the clipboard offset from the originals, recreating internal wiring. Local —
  // nodes + wires get client ids and render instantly; persisted on Save (#122).
  function pasteClipboard() {
    if (!graph || editLocked || !clipboard.nodes.length) return;
    const g = graph;
    const OFFSET = 48;
    const realOf = new Map<number, number>();
    const created: number[] = [];
    for (const src of clipboard.nodes) {
      const id = nextNodeId(g);
      realOf.set(src.id, id);
      created.push(id);
      const config = structuredClone(src.config) as Record<string, unknown>;
      g.nodes.push({ id, kind: src.kind, x: src.x + OFFSET, y: src.y + OFFSET, config, memoize: src.memoize } as Node);
    }
    for (const e of clipboard.edges) {
      if (!realOf.has(e.from) || !realOf.has(e.to)) continue; // only fully-internal wires
      g.edges.push({ from: realOf.get(e.from)!, from_port: e.from_port, to: realOf.get(e.to)!, to_port: e.to_port });
    }
    selectedIds = new Set(created);
    selectedNodeId = created.at(-1) ?? null;
    markDirty();
  }
  function removeEdge(idx: number) {
    if (!graph || editLocked) return;
    const removed = graph.edges[idx];
    if (!removed) return;
    // Local edit — the canvas updates instantly; persisted on Save (#122).
    graph.edges.splice(idx, 1);
    const vk = edgeViaKey(removed);
    if (edgeVias[vk]) { delete edgeVias[vk]; saveEdgeVias(); }
    markDirty();
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
    cursor = { x: node.x + nodeW(node), y: outPortY(node, port) };
  }
  async function clickInPort(node: Node, port: string) {
    if (!graph || editLocked) return;
    const existing = graph.edges.findIndex((e) => e.to === node.id && e.to_port === port);
    // Clicking a wired input unwires it. The exception: exec_in with an output already
    // picked — the engine accepts converging exec paths, so that click adds a wire
    // instead. Data ports stay single-fed.
    const addingFanIn = port === EXEC_IN_NAME && pending !== null;
    if (existing >= 0 && !addingFanIn) {
      await removeEdge(existing);
      return;
    }
    if (!pending) {
      toast.info("Pick an output port first, then this input.");
      return;
    }
    const from = pending;
    const duplicate = graph.edges.some(
      (e) => e.to === node.id && e.to_port === port && e.from === from.node && e.from_port === from.port,
    );
    if (duplicate) {
      pending = null;
      toast.info("Those pins are already wired.");
      return;
    }
    const newEdge: Edge = { from: from.node, from_port: from.port, to: node.id, to_port: port };
    pending = null;
    graph.edges.push(newEdge); // local + instant; persists on Save (#122)
    // Create Dictionary Row (#113): wiring a Dictionary Schema into `schema` bakes its columns
    // onto the row node so each column shows up as a typed input.
    if (node.kind === "dict_make_row" && port === "schema") bakeSchemaOnto(node, from.node);
    markDirty();
  }
  // Copy a Dictionary Schema node's columns onto a Create Dictionary Row node (#113), so its
  // per-column input ports appear (effectiveKind reads config.columns). No-op unless the source
  // is a dict_schema.
  function bakeSchemaOnto(makeRow: Node, sourceId: number) {
    const src = nodeById.get(sourceId);
    if (!src || src.kind !== "dict_schema") return;
    const cols = Array.isArray(src.config?.columns) ? $state.snapshot(src.config.columns) : [];
    makeRow.config = { ...makeRow.config, columns: structuredClone(cols) };
  }
  // ---- Dictionary Schema node: inline columns editor (#113) -----------------
  const CELL_TYPE_OPTS = [
    { value: "text", label: "Text" }, { value: "num", label: "Number" }, { value: "bool", label: "Bool" },
    { value: "camera", label: "Camera" }, { value: "recipient", label: "Recipient" },
  ];
  function schemaCols(node: Node): { name: string; type: string }[] {
    return Array.isArray(node.config?.columns) ? ($state.snapshot(node.config.columns) as { name: string; type: string }[]) : [];
  }
  // Set a schema's columns, then re-bake into every connected Create Dictionary Row so its inputs
  // follow the schema live.
  function setSchemaColumns(node: Node, columns: { name: string; type: string }[]) {
    node.config = { ...node.config, columns };
    for (const e of graph?.edges ?? []) {
      if (e.from === node.id && e.to_port === "schema") {
        const mr = nodeById.get(e.to);
        if (mr && mr.kind === "dict_make_row") mr.config = { ...mr.config, columns: structuredClone(columns) };
      }
    }
    markDirty();
  }
  function addSchemaColumn(node: Node) {
    setSchemaColumns(node, [...schemaCols(node), { name: `col${schemaCols(node).length + 1}`, type: "text" }]);
  }
  function editSchemaColumn(node: Node, i: number, patch: Partial<{ name: string; type: string }>) {
    setSchemaColumns(node, schemaCols(node).map((c, j) => (j === i ? { ...c, ...patch } : c)));
  }
  function removeSchemaColumn(node: Node, i: number) {
    setSchemaColumns(node, schemaCols(node).filter((_, j) => j !== i));
  }
  // Update the node's config in LOCAL state instantly. Nothing persists until Save (#122) — no
  // network, no reload (the old `await reload()` on every change is what wiped your input mid-type).
  function saveNodeConfig(n: Node, config: Record<string, unknown>) {
    if (!graph) return;
    n.config = config;
    markDirty();
  }

  // Live-edit a single config field as the user types: update local state instantly. Persists on
  // Save (#122) — no per-keystroke network write.
  function liveSaveConfig(n: Node, key: string, value: unknown) {
    if (!graph) return;
    n.config = { ...n.config, [key]: value };
    markDirty();
  }
  function saveNodeMemoize(n: Node, memoize: boolean) {
    if (!graph) return;
    n.memoize = memoize; // local + instant; persists on Save (#122)
    markDirty();
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
  // Multi-touch: track every active pointer so we can detect a 2-finger pinch/pan. `pinching`
  // latches until the last finger lifts so a trailing finger doesn't jerk into a pan/drag.
  let pointers = new Map<number, { x: number; y: number }>();
  let gesture: { d: number; cx: number; cy: number; panX: number; panY: number; zoom: number } | null = null;
  let pinching = false;

  // Screen → world (the <g> transform is translate(pan) scale(zoom)).
  function worldPoint(e: PointerEvent | WheelEvent) {
    const r = svgEl!.getBoundingClientRect();
    return { x: (e.clientX - r.left - pan.x) / zoom, y: (e.clientY - r.top - pan.y) / zoom };
  }
  // Node click selection: shift toggles this node in the set; a plain click on an unselected node
  // replaces the selection; a plain click within an existing multi-selection keeps it (so the group
  // can be dragged). `selectedNodeId` (the primary) always follows the clicked node for the inspector.
  function onNodeSelect(e: PointerEvent, n: Node) {
    if (e.shiftKey) {
      const next = new Set(selectedIds);
      if (next.has(n.id)) next.delete(n.id);
      else next.add(n.id);
      selectedIds = next;
      selectedNodeId = n.id;
    } else if (!selectedIds.has(n.id)) {
      selectOnly(n.id);
    } else {
      selectedNodeId = n.id;
    }
  }
  function startDrag(e: PointerEvent, n: Node) {
    onNodeSelect(e, n); // select (shift toggles; plain click replaces, or keeps a group to drag)
    if (editLocked) return; // …but no moving while the graph is active (running/armed)
    // Pan tool: a drag starting on a node pans the canvas instead of moving the node.
    if (editorMode === "pan") {
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      panning = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
      svgEl?.setPointerCapture?.(e.pointerId);
      e.preventDefault();
      return;
    }
    // Link / Break tools: node body is inert (ports still tap to wire) so taps are precise.
    if (editorMode === "link" || editorMode === "break") return;
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size >= 2) return; // a 2nd finger → let the move handler pinch, don't drag
    e.preventDefault();
    svgEl?.setPointerCapture?.(e.pointerId);
    const wp = worldPoint(e);
    const base = { id: n.id, offX: wp.x - n.x, offY: wp.y - n.y };
    // Group drag: a node in a multi-selection carries the rest of the set; otherwise a comment
    // carries the nodes it encloses (its historical behaviour).
    let enclosedIds: number[] = [];
    if (selectedIds.has(n.id) && selectedIds.size > 1) {
      enclosedIds = [...selectedIds].filter((id) => id !== n.id);
    } else if (n.kind === 'comment' && graph) {
      const cw = commentW(n), ch = commentH(n);
      enclosedIds = graph.nodes
        .filter(r => r.kind !== 'comment'
          && r.x + NODE_W / 2 >= n.x && r.x + NODE_W / 2 <= n.x + cw
          && r.y + nodeHeight(r) / 2 >= n.y && r.y + nodeHeight(r) / 2 <= n.y + ch)
        .map(r => r.id);
    }
    drag = { ...base, enclosedIds };
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
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size >= 2) { panning = null; marquee = null; return; } // 2nd finger → pinch
    if (editorMode === "move") {
      // Move tool: empty-canvas drag = rubber-band marquee select (Unreal-style). Shift keeps
      // the current selection to add to it; a plain drag starts fresh.
      if (!e.shiftKey) clearSelection();
      const wp = worldPoint(e);
      marquee = { x0: wp.x, y0: wp.y, x1: wp.x, y1: wp.y };
    } else {
      // Pan / link / break tools: empty-canvas drag pans the canvas.
      clearSelection();
      panning = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
    }
    svgEl?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  let _canvasPendingMove: PointerEvent | null = null;
  let _canvasMoveRaf: number | null = null;

  function onCanvasMove(e: PointerEvent) {
    // Update the live pointer position synchronously (the rAF batch below only keeps the
    // latest event, but a pinch needs BOTH fingers' current positions).
    if (pointers.has(e.pointerId)) pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    _canvasPendingMove = e;
    if (_canvasMoveRaf !== null) return;
    _canvasMoveRaf = requestAnimationFrame(() => {
      _canvasMoveRaf = null;
      const ev = _canvasPendingMove;
      if (ev) { _canvasPendingMove = null; _onCanvasMove(ev); }
    });
  }

  function _onCanvasMove(e: PointerEvent) {
    // Two fingers down → pinch-zoom + pan about the centroid; abort any single-finger op.
    if (pointers.size >= 2) {
      drag = null; panning = null; dragVia = null; commentResize = null;
      const [a, b] = [...pointers.values()];
      const d = Math.hypot(a.x - b.x, a.y - b.y) || 1;
      const r = svgEl!.getBoundingClientRect();
      const cx = (a.x + b.x) / 2 - r.left;
      const cy = (a.y + b.y) / 2 - r.top;
      if (!gesture) {
        gesture = { d, cx, cy, panX: pan.x, panY: pan.y, zoom };
      } else {
        const nz = clampZoom(gesture.zoom * (d / gesture.d));
        // keep the world point that was under the initial centroid pinned to the current one
        const wx = (gesture.cx - gesture.panX) / gesture.zoom;
        const wy = (gesture.cy - gesture.panY) / gesture.zoom;
        pan = { x: cx - wx * nz, y: cy - wy * nz };
        zoom = nz;
      }
      pinching = true;
      return;
    }
    if (pinching) return; // a lone trailing finger after a pinch — ignore until all lift
    hoverWorld = worldPoint(e); // track the cursor for the `n`-to-add-here shortcut
    hoverClient = { x: e.clientX, y: e.clientY };
    if (pending) cursor = worldPoint(e); // drag the open wire end with the cursor
    if (marquee) {
      const wp = worldPoint(e);
      marquee = { ...marquee, x1: wp.x, y1: wp.y };
      return;
    }
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
          // Reroute dots (wire bends) travel with the group: shift the vias of any wire whose
          // BOTH endpoints are moving, so the lasso moves nodes + their wiring together (#123).
          if (dx || dy) {
            const moving = new Set<number>([drag.id, ...drag.enclosedIds]);
            for (const edge of graph.edges) {
              if (!moving.has(edge.from) || !moving.has(edge.to)) continue;
              const key = edgeViaKey(edge);
              const pts = edgeVias[key];
              if (pts?.length) edgeVias[key] = pts.map((p) => ({ x: p.x + dx, y: p.y + dy }));
            }
          }
        }
      }
    }
  }
  async function onCanvasUp(e?: PointerEvent) {
    if (e) {
      pointers.delete(e.pointerId);
      svgEl?.releasePointerCapture?.(e.pointerId);
    }
    if (pointers.size < 2) gesture = null; // pinch ended (or a finger lifted)
    if (pointers.size === 0) pinching = false; // all fingers up → re-enable single-finger ops
    if (marquee) {
      const b = {
        x0: Math.min(marquee.x0, marquee.x1), y0: Math.min(marquee.y0, marquee.y1),
        x1: Math.max(marquee.x0, marquee.x1), y1: Math.max(marquee.y0, marquee.y1),
      };
      const additive = !!e?.shiftKey;
      const hit = (graph?.nodes ?? []).filter((n) => nodeInBox(n, b)).map((n) => n.id);
      const next = new Set(additive ? selectedIds : []);
      for (const id of hit) next.add(id);
      selectedIds = next;
      selectedNodeId = hit.length ? hit[hit.length - 1] : additive ? selectedNodeId : null;
      marquee = null;
      return;
    }
    if (dragVia) {
      dragVia = null;
      saveEdgeVias(); // persist the bend after a drag
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
      drag = null;
      markDirty(); // positions already updated locally during the drag; persists on Save (#122)
      saveEdgeVias(); // capture any wire bends that traveled with a group drag (#123)
    }
  }
  // Open the add-node menu, dropping the new node at the canvas center (touch has no cursor).
  function openAddNodeMenu(e: MouseEvent) {
    if (svgEl) {
      const r = svgEl.getBoundingClientRect();
      dropAt = { x: (r.width / 2 - pan.x) / zoom, y: (r.height / 2 - pan.y) / zoom };
    }
    contextPos = { x: (e.currentTarget as HTMLElement).getBoundingClientRect().left, y: 60 };
    contextPending = null;
    contextOpen = true;
  }
  // Canvas cursor reflects the active tool so the mode is legible on desktop.
  function canvasCursorClass(): string {
    if (panning) return "cursor-grabbing";
    if (editorMode === "link" || editorMode === "break") return "cursor-crosshair";
    return "cursor-grab"; // move + pan both grab the canvas
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
    const maxX = Math.max(...graph.nodes.map((n) => n.x + (isDecoration(n.kind) ? dispW(n) : nodeW(n)))) + 60;
    const maxY = Math.max(...graph.nodes.map((n) => n.y + (isDecoration(n.kind) ? dispH(n) : nodeHeight(n)))) + 60;
    const r = svgEl.getBoundingClientRect();
    const z = clampZoom(Math.min(r.width / (maxX - minX), r.height / (maxY - minY), 1.5));
    zoom = z;
    pan = {
      x: (r.width - (maxX - minX) * z) / 2 - minX * z,
      y: (r.height - (maxY - minY) * z) / 2 - minY * z,
    };
  }

  // The edge tracker's 3 gated events (see docs/ref/workflows.md's "Gated events" section) —
  // keep this in sync with the same literal list in `sentinel-control`'s `sync_tracking_config`
  // and `sentinel-edge`'s job resolution; a divergence here silently drops tracking jobs.
  const TRACKING_EVENTS = ["object_entered", "object_exited", "line_crossed"];
  // Per-job SORT/ByteTrack tuning, exposed on a tracking OnEvent node so different jobs can be
  // tuned independently — edge-calibrated defaults (NOT the algorithm's generic ByteTrack-ish
  // ones), must match `sentinel-edge`'s `default_min_hits`/etc. and `sentinel-control`'s
  // `sync_tracking_config` fallback constants (`sentinel-edge/src/tracking.rs`,
  // `sentinel-control/src/domain/workflows.rs`) — all three move together if ever re-tuned.
  const TRACKING_TUNING_DEFAULTS: Record<string, string> = {
    min_hits: "2",
    max_age: "25",
    iou_thresh: "0.3",
    high_thresh: "0.5",
    low_thresh: "0.1",
  };
  const TRACKING_TUNING_HELP: Record<string, string> = {
    min_hits: "Consecutive matched detections before a track is confirmed (fires Entered).",
    max_age: "Detection cycles a track may go unmatched before it exits (edge cadence: ~400ms/cycle by default).",
    iou_thresh: "Minimum box overlap (0–1) required to match a detection to an existing track.",
    high_thresh: "Detection confidence (0–1) required to match or start a track.",
    low_thresh: "Detection confidence (0–1) that can still recover a lost track, but never starts one.",
  };
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
    { value: "schedule", label: "Schedule (calendar)" },
  ];
  const CONC_OPTS = [
    { value: "skip", label: "Skip if running" },
    { value: "queue", label: "Queue (max N)" },
    { value: "concurrent", label: "Concurrent (max N)" },
  ];
  const INSTANCE_OPTS = [
    { value: "single", label: "Single (one at a time)" },
    { value: "per_camera", label: "Per camera" },
  ];
  function labelOf(opts: { value: string; label: string }[], v: string) {
    return opts.find((o) => o.value === v)?.label ?? v;
  }
  // Instance scope (#95): the API sends the enum as "Single"/"PerCamera"; the selector speaks
  // "single"/"per_camera". Default (missing) is Single — byte-for-byte the pre-instances behaviour.
  function instanceScopeKind(g: Graph): "single" | "per_camera" {
    return g.instance_scope === "PerCamera" ? "per_camera" : "single";
  }
  function instanceScopeHint(g: Graph): string {
    return instanceScopeKind(g) === "per_camera"
      ? "Each camera gets its own run context, window and clip — two cameras record and stop independently."
      : "One run context per graph. Switch to Per camera if this graph records or tracks multiple cameras at once.";
  }
</script>

<svelte:window
  onkeydown={onWindowKey}
  onbeforeunload={(e) => {
    if (dirty) {
      e.preventDefault();
      e.returnValue = ""; // triggers the browser's native "unsaved changes" prompt
    }
  }}
/>

<div class="h-full">
  {#if loading}
    <div class="flex h-full items-center justify-center">
      <Loader label="Loading workflows…" />
    </div>

    <!-- ════════════════════ LIST ════════════════════ -->
  {:else if view === "list"}
    <WorkflowsList
      {graphs}
      {templates}
      {templatesLoading}
      {usingTemplateId}
      {allGroups}
      {collapsedGroups}
      {activity}
      {isMaster}
      {wfSearch}
      {wfGroupFilter}
      bind:listTab
      {createWorkflow}
      {showTemplatesTab}
      {cloneTemplate}
      {reloadDictionaries}
      {toggleGroup}
      {onSearchInput}
      {setGroupFilter}
      {clearListFilters}
      {openGraph}
      {runNow}
      {toggleEnabled}
      {toggleTemplate}
      {toggleGlobal}
      {duplicateWorkflow}
      {openRuns}
      {openDelete}
      {runStatus}
      {dotState}
      {triggerSummary}
      {graphHasUnfilled}
      {listRunTitle}
      {listRunDisabled}
    />

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

      <!-- row 1: editor + the tabbed right rail (there is no left rail — variables live in it) -->
      <div class="flex min-h-0 flex-1 gap-1">
        <!-- col 0: the node editor -->
        <div class="relative min-w-0 flex-1">
          <div
            class="absolute inset-0 touch-none overflow-hidden rounded-xl border border-border bg-well"
            style="background-image: radial-gradient(circle at center, var(--wf-grid-strong) {Math.max(1, 1.6 * zoom)}px, transparent 0), radial-gradient(circle at center, var(--wf-grid-weak) {Math.max(0.5, 1 * zoom)}px, transparent 0); background-size: {90 * zoom}px {90 * zoom}px, {18 * zoom}px {18 * zoom}px; background-position: {pan.x}px {pan.y}px, {pan.x}px {pan.y}px;"
            role="application"
            ondragover={onCanvasDragOver}
            ondrop={onCanvasDrop}
          >
            <svg
              bind:this={svgEl}
              use:passiveWheel
              data-canvas
              class="block size-full touch-none {canvasCursorClass()}"
              onpointerdown={onCanvasDown}
              onpointermove={onCanvasMove}
              onpointerup={onCanvasUp}
              onpointerleave={onCanvasUp}
              onpointercancel={onCanvasUp}
              role="presentation"
            >
              <defs>
                <filter id="nodeGlow" x="-40%" y="-40%" width="180%" height="180%">
                  <feDropShadow dx="0" dy="0" stdDeviation="5" style="flood-color: var(--wf-node-glow)" flood-opacity="0.5" />
                </filter>
                <!-- Resting elevation: a soft grounded shadow so cards float above the canvas. -->
                <filter id="nodeShadow" x="-30%" y="-30%" width="160%" height="160%">
                  <feDropShadow dx="0" dy="2" stdDeviation="4" style="flood-color: var(--wf-node-shadow)" flood-opacity="1" />
                </filter>
              </defs>

              <g transform="translate({pan.x} {pan.y}) scale({zoom})">
              <!-- rubber-band marquee (move tool, drag on empty canvas) -->
              {#if marquee}
                <rect
                  x={Math.min(marquee.x0, marquee.x1)}
                  y={Math.min(marquee.y0, marquee.y1)}
                  width={Math.abs(marquee.x1 - marquee.x0)}
                  height={Math.abs(marquee.y1 - marquee.y0)}
                  class="pointer-events-none fill-primary/10 stroke-primary"
                  stroke-width={1 / zoom}
                  stroke-dasharray="{4 / zoom} {3 / zoom}"
                  rx={2 / zoom}
                />
              {/if}
              <!-- comment boxes (behind edges and regular nodes) -->
              {#each graph.nodes.filter(cn => cn.kind === 'comment') as n (n.id)}
                {@const cw = commentW(n)}
                {@const ch = commentH(n)}
                {@const col = String(n.config.color ?? COMMENT_DEFAULT_COLOR)}
                {@const sel = isSelected(n.id)}
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
                  {@const x1 = fromN.x + nodeW(fromN)}
                  {@const y1 = outPortY(fromN, e.from_port)}
                  {@const x2 = toN.x}
                  {@const y2 = inPortY(toN, e.to_port)}
                  {@const eLive = nodeLive(e.to)}
                  {@const eColor = portColorOf(fromN, e.from_port, "out")}
                  {@const eColorTo = portColorOf(toN, e.to_port, "in")}
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
                    <!-- Wire gradient: flows from the source port's type colour to the
                         target's, so a wire reads as a bridge between two typed pins. -->
                    <linearGradient
                      id="wire-{idx}"
                      gradientUnits="userSpaceOnUse"
                      x1={x1}
                      y1={y1}
                      x2={x2}
                      y2={y2}
                    >
                      <stop offset="0" style="stop-color: {eColor === portColor.exec ? 'var(--wf-exec)' : eColor}" />
                      <stop offset="1" style="stop-color: {eColorTo === portColor.exec ? 'var(--wf-exec)' : eColorTo}" />
                    </linearGradient>
                    <!-- wide transparent hit area; break tool / alt+click deletes wire; drag adds/moves bend -->
                    <path
                      d={pathD}
                      fill="none"
                      stroke="transparent"
                      stroke-width={isTouch || editorMode === "break" ? 28 : 20}
                      class="cursor-crosshair"
                      onpointerdown={(ev) => {
                        ev.stopPropagation();
                        if (editorMode === "break" || ev.altKey) { removeEdge(idx); return; }
                        if (editorMode !== "move") return; // bends are a move-tool gesture
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
                      stroke="url(#wire-{idx})"
                      stroke-width={eLive ? 3 : 2}
                      stroke-linecap="round"
                      stroke-opacity={eLive ? 1 : 0.7}
                      class="pointer-events-none transition-colors {editorMode === 'break' || hoveredEdgeKey === vk ? 'stroke-destructive/70' : ''} {eLive ? 'node-running' : ''}"
                      stroke-dasharray={editorMode === "break" ? "6 4" : undefined}
                      role="presentation"
                    />
                    <!-- bend handles; alt+click removes that via-point -->
                    {#each vias as via, vi}
                      <circle
                        cx={via.x}
                        cy={via.y}
                        r="5"
                        fill={eColor}
                        style="stroke: var(--wf-dot-ring)"
                        stroke-width="1.5"
                        stroke-opacity="0.4"
                        class="cursor-grab"
                        onpointerdown={(ev) => {
                          ev.stopPropagation();
                          if (ev.altKey) {
                            edgeVias[vk] = vias.filter((_, j) => j !== vi);
                            if (edgeVias[vk].length === 0) delete edgeVias[vk];
                            saveEdgeVias();
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
                {@const nw = nodeW(n)}
                {@const sel = isSelected(n.id)}
                {@const live = nodeLive(n.id)}
                {@const invalid = requiredUnfilled(n).length > 0}
                {@const CatIcon = categoryIcon(n)}
                <!-- One transform moves the whole node; inner coords are local,
                     so dragging only updates this single attribute (no per-port
                     recompute). -->
                <g transform="translate({n.x} {n.y})" filter={sel || live ? "url(#nodeGlow)" : "url(#nodeShadow)"}>
                  <rect
                    width={nw}
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
                        width={nw}
                        height={h}
                        rx="8"
                        fill="none"
                        stroke-width="3"
                        class="node-run-glow stroke-primary"
                      />
                      <rect
                        width={nw}
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
                    d="M0 8 a8 8 0 0 1 8 -8 h{nw - 16} a8 8 0 0 1 8 8 v{HEAD_H - 8} h-{nw} z"
                    fill={headerColorFor(n)}
                    fill-opacity={n.kind === "emit_event" || n.kind === "on_event" ? 0.32 : 0.18}
                    class="pointer-events-none"
                  />
                  <!-- Drag handle covers the whole node body; ports render after
                       this rect, so they stay clickable on top of it. -->
                  <rect
                    width={nw}
                    height={h}
                    rx="8"
                    fill="transparent"
                    class="cursor-move"
                    onpointerdown={(ev) => startDrag(ev, n)}
                    role="presentation"
                  />
                  <!-- Category glyph — a face for each node family, tinted to match the header. -->
                  <g
                    transform="translate(10 {HEAD_H / 2 - 7.5})"
                    style="color: {headerColorFor(n)}"
                    class="pointer-events-none"
                  >
                    <CatIcon size={15} strokeWidth={2.25} />
                  </g>
                  <text
                    x="32"
                    y="17"
                    class="pointer-events-none select-none fill-foreground"
                    font-size="11"
                    font-weight="600"
                  >
                    {nodeTitle(n, k)}
                  </text>
                  <!-- No on-canvas delete badge: select + press Delete/Backspace to remove a node
                       (see the keydown handler), or use the inspector's Delete button. -->
                  <!-- Category-tinted header underline — each node family gets a
                       coloured seam under its title (the strongest at-a-glance cue). -->
                  <line
                    x1="0"
                    y1={HEAD_H}
                    x2={nw}
                    y2={HEAD_H}
                    class="pointer-events-none"
                    style="stroke: {headerColorFor(n)}"
                    stroke-opacity="0.55"
                    stroke-width="1.5"
                  />
                  {#if isCompareOrLogic(n.kind)}
                    {@const op = opLabel(n.kind, n.config)}
                    {@const aVal = exprOperand(n.id, "a", n.config)}
                    {@const bVal = exprOperand(n.id, "b", n.config)}
                    {@const ey = HEAD_H + EXPR_H / 2}
                    <text
                      x={nw / 2}
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
                      x={nw - 10}
                      y={ey + 5}
                      text-anchor="end"
                      font-size="8"
                      class="pointer-events-none select-none fill-muted-foreground/70"
                    >{bVal}</text>
                  {/if}
                  {#if invalid}
                    <!-- required input(s) not filled — a red dot, the logo's signal -->
                    <circle cx={nw - 9} cy="9" r="5.5" class="pointer-events-none fill-destructive/20" />
                    <circle cx={nw - 9} cy="9" r="3" class="fill-destructive">
                      <title>Required input not filled: {requiredUnfilled(n).map((p) => p.name).join(", ")}</title>
                    </circle>
                  {/if}

                  <!-- exec (control-flow) pins — white. exec_in on the left header.
                       Pure data/logic nodes (comparisons, conversions) have no exec pins. -->
                  {#if !isPure(k)}
                  <circle
                    cx="0"
                    cy={HEAD_H / 2}
                    r={portR}
                    style="fill: {wiredInto(n.id, EXEC_IN_NAME) ? 'var(--wf-exec)' : 'var(--wf-port-empty)'}; stroke: var(--wf-exec)"
                    stroke-width="2"
                    class="cursor-pointer wf-port"
                    onclick={() => clickInPort(n, EXEC_IN_NAME)}
                    role="presentation"
                  />
                  {/if}
                  <!-- exec outputs: single exec_out stays at the header right edge;
                       multi-exec (arms/branches) all live in the node body so the header
                       area stays clean and the arms read left-to-right with their labels. -->
                  {#each execOutPorts(k) as p, i}
                    {@const multi = hasMultipleExecOuts(k)}
                    {@const row = isForEach(k) && p.name === "completed" ? forEachCompletedRow(k) : i}
                    {@const ey = multi ? HEAD_H + row * ROW_H + ROW_H / 2 : HEAD_H / 2}
                    {@const isP = pending && pending.node === n.id && pending.port === p.name}
                    <circle
                      cx={nw}
                      cy={ey}
                      r={isP ? portR + 1 : portR}
                      style="fill: {isP ? 'var(--wf-exec)' : 'var(--wf-port-empty)'}; stroke: var(--wf-exec)"
                      stroke-width="2"
                      class="cursor-pointer transition-all wf-port"
                      onclick={() => clickOutPort(n, p.name)}
                      role="presentation"
                    />
                    {#if multi}
                      <text
                        x={nw - 9}
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
                      r={portR}
                      style="fill: {wired ? portColor[p.type] : 'var(--wf-port-empty)'}; stroke: {need ? 'var(--destructive)' : portColor[p.type]}"
                      stroke-width="2"
                      class="cursor-pointer wf-port"
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
                      cx={nw}
                      cy={dy}
                      r={isPending ? portR + 1 : portR}
                      style="fill: {isPending ? pc : 'var(--wf-port-empty)'}"
                      stroke={pc}
                      stroke-width="2"
                      class="cursor-pointer transition-all wf-port"
                      onclick={() => clickOutPort(n, p.name)}
                      role="presentation"
                    />
                    <text
                      x={nw - 11}
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
                      width={nw - 16}
                      height={18}
                      rx="4"
                      class={armed ? "fill-primary/15" : "fill-muted/20"}
                    />
                    <text
                      x={nw / 2}
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

                  <!-- Display live sinks (Text / Gauge): the value readout below the ports. -->
                  {#if n.kind === "text"}
                    {@const vy = portsBottom(n) + 4}
                    {@const bg = String(n.config?.bg_color ?? "#1f2937")}
                    {@const fg = String(n.config?.text_color ?? "#e5e7eb")}
                    {@const val = String(sinkValue(n) ?? "")}
                    <clipPath id="textval-clip-{n.id}"><rect x={8} y={vy} width={nw - 16} height={TEXT_DISPLAY_H - 6} rx="6" /></clipPath>
                    <rect x={8} y={vy} width={nw - 16} height={TEXT_DISPLAY_H - 6} rx="6" fill={bg} />
                    <text x={16} y={vy + (TEXT_DISPLAY_H - 6) / 2 + 4} font-size="13" fill={fg}
                      clip-path="url(#textval-clip-{n.id})" class="pointer-events-none select-none">{val || "—"}</text>
                  {:else if n.kind === "gauge"}
                    {@const col = String(n.config?.color ?? "#22c55e")}
                    {@const frac = gaugeFraction(n)}
                    {@const cx = nw / 2}
                    {@const cy = portsBottom(n) + 64}
                    {@const r = 38}
                    {@const a = Math.PI * (1 - frac)}
                    {@const ex = cx + r * Math.cos(a)}
                    {@const ey = cy - r * Math.sin(a)}
                    <path d="M {cx - r} {cy} A {r} {r} 0 0 1 {cx + r} {cy}" fill="none" class="stroke-muted" stroke-width="7" stroke-linecap="round" />
                    {#if frac > 0}
                      <path d="M {cx - r} {cy} A {r} {r} 0 0 1 {ex} {ey}" fill="none" stroke={col} stroke-width="7" stroke-linecap="round" />
                    {/if}
                    <text x={cx} y={cy - 4} text-anchor="middle" font-size="16" font-weight="700" class="pointer-events-none select-none fill-foreground">{String(sinkValue(n) ?? "—")}</text>
                    {#if n.config?.label}
                      <text x={cx} y={cy + 14} text-anchor="middle" font-size="10" class="pointer-events-none select-none fill-muted-foreground">{String(n.config.label)}</text>
                    {/if}
                  {/if}
                </g>
              {/each}

              <!-- display cards (Title / Paragraph / Markdown) — foreground, custom bodies, no ports -->
              {#each displayNodes as n (n.id)}
                {@const w = dispW(n)}
                {@const h = dispH(n)}
                {@const sel = isSelected(n.id)}
                <g transform="translate({n.x} {n.y})">
                  {#if n.kind === "title"}
                    {@const ts = titleFontSize(n)}
                    {@const col = String(n.config.color ?? "#e5e7eb")}
                    <rect width={w} height={h} fill="transparent" class="cursor-move"
                      onpointerdown={(ev) => startDrag(ev, n)} role="presentation" />
                    <text x="0" y={Math.round(ts * 0.82) + 6} font-size={ts} font-weight="700"
                      fill={col} class="pointer-events-none select-none">{String(n.config.text ?? "Title") || "Title"}</text>
                    {#if sel}
                      <rect x="-6" y="-2" width={w + 12} height={h + 4} rx="6" fill="none"
                        class="pointer-events-none stroke-primary" stroke-width="1.5" stroke-dasharray="4 3" />
                    {/if}
                  {:else if n.kind === "paragraph"}
                    {@const col = String(n.config.color ?? "#9ca3af")}
                    {@const lines = wrapText(String(n.config.text ?? ""), w, 13)}
                    <clipPath id="disp-clip-{n.id}"><rect width={w} height={h} rx="10" /></clipPath>
                    <rect width={w} height={h} rx="10" stroke-width={sel ? 2 : 1}
                      class="cursor-move fill-background {sel ? 'stroke-primary' : 'stroke-border'}"
                      onpointerdown={(ev) => startDrag(ev, n)} role="presentation" />
                    <text x="12" y="24" font-size="13" fill={col}
                      clip-path="url(#disp-clip-{n.id})" class="pointer-events-none select-none">
                      {#each lines as ln, i}
                        <tspan x="12" dy={i === 0 ? 0 : 20}>{ln || " "}</tspan>
                      {/each}
                    </text>
                    <polygon points="{w - 16},{h} {w},{h - 16} {w},{h}"
                      class="cursor-se-resize fill-muted-foreground/50"
                      onpointerdown={(ev) => { selectedNodeId = n.id; startCommentResize(ev, n); }} role="presentation" />
                  {:else if n.kind === "markdown"}
                    {@const mls = mdLines(String(n.config.text ?? ""), w)}
                    <clipPath id="disp-clip-{n.id}"><rect width={w} height={h} rx="10" /></clipPath>
                    <rect width={w} height={h} rx="10" stroke-width={sel ? 2 : 1}
                      class="cursor-move fill-background {sel ? 'stroke-primary' : 'stroke-border'}"
                      onpointerdown={(ev) => startDrag(ev, n)} role="presentation" />
                    <g clip-path="url(#disp-clip-{n.id})" class="pointer-events-none">
                      {#each mls as ml}
                        {#if ml.bullet}
                          <circle cx="16" cy={ml.y - 4} r="2" class="fill-muted-foreground" />
                        {/if}
                        <text x={12 + ml.indent} y={ml.y} font-size={ml.size} font-weight={ml.bold ? 700 : 400}
                          class="select-none {ml.bold ? 'fill-foreground' : 'fill-muted-foreground'}">{ml.text}</text>
                      {/each}
                    </g>
                    <polygon points="{w - 16},{h} {w},{h - 16} {w},{h}"
                      class="cursor-se-resize fill-muted-foreground/50"
                      onpointerdown={(ev) => { selectedNodeId = n.id; startCommentResize(ev, n); }} role="presentation" />
                  {/if}
                </g>
              {/each}

              <!-- wire being drawn: from the chosen output port to the cursor -->
              {#if pending}
                {@const pn = nodeById.get(pending.node)}
                {#if pn}
                  {@const px = pn.x + nodeW(pn)}
                  {@const py = outPortY(pn, pending.port)}
                  {@const pColor = portColorOf(pn, pending.port, "out")}
                  <path
                    d={`M ${px} ${py} C ${px + 60} ${py}, ${cursor.x - 60} ${cursor.y}, ${cursor.x} ${cursor.y}`}
                    fill="none"
                    style="stroke: {pColor === portColor.exec ? 'var(--wf-exec)' : pColor}"
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
            <Button class="shadow-lg" disabled={editLocked} onclick={openAddNodeMenu}>
              <Plus class="size-4" /> Add node
            </Button>
          </div>

          <!-- left edge: touch tool palette (Move / Pan / Link / Break) -->
          <div class="absolute left-3 top-1/2 flex -translate-y-1/2 flex-col gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
            {#each [
              { mode: "move", Icon: MousePointer2, label: "Move / select (V)", lock: true },
              { mode: "pan", Icon: Hand, label: "Pan (H)", lock: false },
              { mode: "link", Icon: Link2, label: "Link ports (L)", lock: true },
              { mode: "break", Icon: Scissors, label: "Break link (B)", lock: true },
            ] as t (t.mode)}
              <Button
                variant="ghost"
                size="icon"
                class="size-8 {editorMode === t.mode ? 'bg-primary/15 text-primary' : ''}"
                title={t.label}
                disabled={t.lock && editLocked}
                onclick={() => { editorMode = t.mode as typeof editorMode; pending = null; }}
              >
                <t.Icon class="size-4" />
              </Button>
            {/each}
          </div>

          <!-- on-screen Print messages (Unreal-style debug text, newest on top) -->
          {#if screenMsgs.length}
            <div class="pointer-events-none absolute left-3 top-16 z-10 flex max-w-sm flex-col gap-1.5">
              {#each screenMsgs as msg (msg.id)}
                <div
                  class="flex items-center gap-2 rounded-lg border bg-hud px-3 py-1.5 font-mono text-xs shadow-lg backdrop-blur duration-200 animate-in fade-in slide-in-from-left-2 {screenMsgClass(msg.level)}"
                >
                  <Printer class="size-3 shrink-0" />
                  <span class="truncate">{msg.message}</span>
                </div>
              {/each}
            </div>
          {/if}

          <!-- top-center: save + transport + graph state (Enabled / Debug / Orphans) -->
          <div class="absolute left-1/2 top-3 flex -translate-x-1/2 items-center gap-2">
            <!-- save (canvas edits persist here; metadata persists as you type) -->
            <div class="flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
              <Button
                variant="ghost"
                size="sm"
                class="press h-8 gap-1.5 px-2.5 {dirty ? 'bg-primary/15 text-primary hover:text-primary' : 'text-muted-foreground'}"
                title={dirty ? "Save changes (⌘S)" : "All changes saved"}
                disabled={saving || !dirty}
                onclick={save}
              >
                {#if saving}<Busy />{:else}<Save class="size-4" />{/if}
                <span class="text-xs font-medium">{saving ? "Saving…" : dirty ? "Save" : "Saved"}</span>
                {#if dirty && !saving}<span class="size-1.5 rounded-full bg-primary" aria-hidden="true"></span>{/if}
              </Button>
            </div>
            <!-- transport -->
            <div class="flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
              <Button
                variant="ghost"
                size="icon"
                class="size-8 text-primary hover:text-primary"
                title={editorRunTitle()}
                disabled={editorRunDisabled()}
                onclick={() => runNow(graph)}
              >
                {#if running}<Busy />{:else}<Play class="size-4" />{/if}
              </Button>
              <Button variant="ghost" size="icon" class="size-8" title="Pause (coming soon)" disabled={!isRunning} onclick={() => comingSoon("Pause")}>
                <Pause class="size-4" />
              </Button>
              <Button variant="ghost" size="icon" class="size-8" title="Stop (coming soon)" disabled={!isRunning} onclick={() => comingSoon("Stop")}>
                <Square class="size-4" />
              </Button>
            </div>

            <!-- graph state: Enabled → Debug → Orphans -->
            <div class="flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
              <Button
                variant="ghost"
                size="icon"
                class="size-8 press {graph.enabled ? 'status-live bg-primary/15 text-primary ring-1 ring-inset ring-primary/40 hover:text-primary' : 'text-muted-foreground'}"
                title={enabledTitle()}
                onclick={() => toggleEnabled(graph, !graph.enabled)}
              >
                <Power class="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                class="size-8 press {graph.debug ? 'bg-primary/15 text-primary hover:text-primary' : 'text-muted-foreground'}"
                title={debugTitle()}
                disabled={editLocked}
                onclick={() => toggleDebug(graph, !graph.debug)}
              >
                <Bug class="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                class="size-8 press {(graph.run_orphans ?? false) ? 'bg-primary/15 text-primary hover:text-primary' : 'text-muted-foreground'}"
                title={orphansTitle()}
                disabled={editLocked}
                onclick={() => toggleRunOrphans(graph, !(graph.run_orphans ?? false))}
              >
                <Unplug class="size-4" />
              </Button>
            </div>
          </div>

          <!-- top-right: zoom / pan -->
          <div
            class="absolute right-3 top-3 flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur"
          >
            <Button variant="ghost" size="icon" class="size-8" title="Zoom out" onclick={() => zoomBy(1 / 1.2)}><ZoomOut class="size-4" /></Button>
            <span class="w-10 text-center text-[11px] tabular-nums text-muted-foreground">{Math.round(zoom * 100)}%</span>
            <Button variant="ghost" size="icon" class="size-8" title="Zoom in" onclick={() => zoomBy(1.2)}><ZoomIn class="size-4" /></Button>
            <Button variant="ghost" size="icon" class="size-8" title="Fit to view" onclick={fitView}><Maximize class="size-4" /></Button>
          </div>

          <!-- bottom-center: wiring hint · selection actions (touch-reachable) · idle hint -->
          <div class="absolute bottom-3 left-1/2 -translate-x-1/2">
            {#if pending}
              <div class="flex items-center gap-2 rounded-full border border-primary/40 bg-hud px-3 py-1 text-xs text-primary shadow-lg backdrop-blur">
                wiring from #{pending.node}.{pending.port} — click an input port
                <button class="text-muted-foreground transition-colors hover:text-foreground" onclick={() => (pending = null)}>cancel</button>
              </div>
            {:else if selectedIds.size > 0 && !editLocked}
              <!-- Selection actions: the copy/duplicate/paste/delete shortcuts are keyboard-only,
                   so surface them as buttons for touch (tablet has no ⌘C/⌘V/Delete). -->
              <div class="flex items-center gap-1 rounded-lg border border-border bg-hud p-1 shadow-lg backdrop-blur">
                <span class="px-1.5 text-[11px] text-muted-foreground">{selectedIds.size} selected</span>
                <Button variant="ghost" size="icon" class="size-8" title="Duplicate (⌘D)" onclick={() => { copySelection(); pasteClipboard(); }}>
                  <CopyPlus class="size-4" />
                </Button>
                <Button variant="ghost" size="icon" class="size-8" title="Copy (⌘C)" onclick={copySelection}>
                  <Copy class="size-4" />
                </Button>
                <Button variant="ghost" size="icon" class="size-8" title="Paste (⌘V)" disabled={!clipboard.nodes.length} onclick={pasteClipboard}>
                  <ClipboardPaste class="size-4" />
                </Button>
                <Button variant="ghost" size="icon" class="size-8 text-muted-foreground hover:text-destructive" title="Delete selection" onclick={deleteSelection}>
                  <Trash2 class="size-4" />
                </Button>
              </div>
            {:else}
              <div class="rounded-full border border-border bg-hud px-3 py-1 text-[11px] text-muted-foreground shadow-lg backdrop-blur">
                Click an output ●, then an input ○ to wire · click a wired input to unwire
              </div>
            {/if}
          </div>
        </div>

        {#if !panelOpen}
          <!-- collapsed: an icon rail — click a tab to reopen straight into it -->
          <div class="flex w-9 shrink-0 flex-col items-center gap-1 rounded-xl border border-border bg-card p-1">
            <button
              class="interactive group flex w-full items-center justify-center rounded-lg py-1.5 text-muted-foreground hover:bg-hover hover:text-foreground"
              title="Show panel"
              aria-label="Show panel"
              onclick={togglePanel}
            >
              <PanelRightOpen class="size-4" />
            </button>
            <div class="my-0.5 h-px w-5 bg-border"></div>
            {#each RAIL_TABS as t (t.id)}
              <button
                class="interactive flex w-full items-center justify-center rounded-lg py-1.5 text-muted-foreground hover:bg-hover hover:text-foreground"
                title={t.label}
                aria-label={t.label}
                onclick={() => selectTab(t.id)}
              >
                <t.icon class="size-4" />
              </button>
            {/each}
          </div>
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

        <!-- col 1: the tabbed rail — Details / Properties / Variables / Runs -->
        <div class="flex min-h-0 shrink-0 flex-col gap-2" style="width: {panelW}px">
          <!-- tab strip: icon + label. Labels truncate as the panel narrows; each tab body
               also carries its own header. -->
          <div class="flex shrink-0 items-center gap-0.5 rounded-xl border border-border bg-card p-1">
            {#each RAIL_TABS as t (t.id)}
              <button
                class={tabClass(t.id)}
                aria-pressed={panelTab === t.id}
                title={t.label}
                aria-label={t.label}
                onclick={() => selectTab(t.id)}
              >
                <t.icon class="size-4 shrink-0" />
                <span class="truncate text-xs font-medium">{t.label}</span>
              </button>
            {/each}
            <div class="mx-0.5 h-5 w-px shrink-0 bg-border"></div>
            <button
              class="interactive flex shrink-0 items-center justify-center rounded-lg px-1.5 py-1.5 text-muted-foreground hover:bg-hover hover:text-foreground"
              title="Hide panel"
              aria-label="Hide panel"
              onclick={togglePanel}
            >
              <PanelRightClose class="size-4" />
            </button>
          </div>

          <!-- tab bodies. Properties HIDES rather than unmounts: RegionPointsEditor
               re-fetches its camera snapshot on mount, so unmounting would cost a network
               round-trip and a visible flicker on every tab switch. -->
          <div class="relative min-h-0 flex-1">

          {#if panelTab === "details"}
          <div class="flex h-full flex-col gap-3 overflow-y-auto">
          <!-- configuration -->
          <div class="space-y-3 rounded-xl border border-border bg-card p-3">
            <div class="flex items-center justify-between gap-2">
              <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Configuration</span>
              <Button variant="ghost" size="icon" class="size-7" title="Delete workflow" onclick={() => openDelete(graph)}>
                <Trash2 class="size-3.5" />
              </Button>
            </div>

            <!-- Name -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3">
              <Label class="text-xs text-muted-foreground">Name</Label>
              <Input class="h-8" disabled={editLocked} value={graph.name} onchange={(e) => renameInline(e.currentTarget.value)} />
            </div>

            <!-- Description -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="pt-2 text-xs text-muted-foreground">Description</Label>
              <Textarea
                rows={2}
                class="min-h-14 resize-y text-sm"
                disabled={editLocked}
                placeholder="What does this workflow do?"
                value={graph.description ?? ""}
                onchange={(e) => saveDescription(e.currentTarget.value)}
              />
            </div>

            <!-- Group -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-center gap-x-3">
              <Label class="text-xs text-muted-foreground">Group</Label>
              <div>
                <Input
                  class="h-8"
                  list="wf-groups"
                  disabled={editLocked}
                  placeholder="Ungrouped"
                  value={graph.group ?? ""}
                  onchange={(e) => saveGroup(e.currentTarget.value)}
                />
                <datalist id="wf-groups">{#each allGroups as grp (grp)}<option value={grp}></option>{/each}</datalist>
              </div>
            </div>

            <!-- Enabled / Debug / Orphans now live as icon toggles at the top of the canvas (#112). -->
            <p class="text-[11px] leading-snug text-muted-foreground">
              Arm, Debug and Run-orphans are the three icon toggles at the top of the canvas.
            </p>

            <!-- Trigger -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="pt-2 text-xs text-muted-foreground">Trigger</Label>
              <div class="space-y-1.5">
                <Select.Root
                  type="single"
                  disabled={editLocked}
                  value={triggerKind(graph.trigger)}
                  onValueChange={(v) => onTriggerKind(graph, v ?? "manual")}
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
                {#if triggerKind(graph.trigger) === "schedule"}
                  {@const s = schedOf(graph.trigger)}
                  <div class="space-y-2 rounded-md border border-border p-2">
                    <Select.Root type="single" disabled={editLocked} value={s.freq} onValueChange={(v) => patchSchedule(graph, { freq: (v ?? "daily") as ScheduleFreq })}>
                      <Select.Trigger class="h-8 w-full">{labelOf(SCHED_FREQ_OPTS, s.freq)}</Select.Trigger>
                      <Select.Content>
                        {#each SCHED_FREQ_OPTS as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                      </Select.Content>
                    </Select.Root>

                    {#if s.freq === "hourly"}
                      <div class="flex items-center gap-2">
                        <span class="text-xs text-muted-foreground">at minute</span>
                        <Input type="number" min="0" max="59" disabled={editLocked} value={s.minute} class="h-8 w-20" onchange={(e) => patchSchedule(graph, { minute: Math.max(0, Math.min(59, Number(e.currentTarget.value))) })} />
                      </div>
                    {:else}
                      {#if s.freq === "weekly"}
                        <div class="flex flex-wrap gap-1">
                          {#each WEEKDAY_LABELS as wd, i}
                            <button type="button" disabled={editLocked} class="interactive rounded px-2 py-1 text-[11px] {s.weekdays.includes(i) ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'}" onclick={() => toggleWeekday(graph, i)}>{wd}</button>
                          {/each}
                        </div>
                      {/if}
                      {#if s.freq === "monthly"}
                        <div class="flex items-center gap-2">
                          <span class="text-xs text-muted-foreground">day</span>
                          <Input type="number" min="1" max="31" disabled={editLocked} value={s.day_of_month} class="h-8 w-20" onchange={(e) => patchSchedule(graph, { day_of_month: Math.max(1, Math.min(31, Number(e.currentTarget.value))) })} />
                        </div>
                      {/if}
                      {#if s.freq === "yearly"}
                        <div class="flex items-center gap-2">
                          <Select.Root type="single" disabled={editLocked} value={String(s.month)} onValueChange={(v) => patchSchedule(graph, { month: Number(v ?? 1) })}>
                            <Select.Trigger class="h-8 w-24">{MONTH_LABELS[(s.month || 1) - 1]}</Select.Trigger>
                            <Select.Content>
                              {#each MONTH_LABELS as m, i}<Select.Item value={String(i + 1)} label={m}>{m}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                          <Input type="number" min="1" max="31" disabled={editLocked} value={s.day_of_month} class="h-8 w-20" onchange={(e) => patchSchedule(graph, { day_of_month: Math.max(1, Math.min(31, Number(e.currentTarget.value))) })} />
                        </div>
                      {/if}
                      <div class="space-y-1">
                        <span class="text-[11px] text-muted-foreground">Times</span>
                        {#each s.times as t, i (i)}
                          <div class="flex items-center gap-1">
                            <Input type="time" disabled={editLocked} value={t} class="h-8" onchange={(e) => setScheduleTime(graph, i, e.currentTarget.value)} />
                            {#if s.times.length > 1}
                              <button type="button" disabled={editLocked} class="interactive px-1 text-xs text-muted-foreground hover:text-destructive" onclick={() => removeScheduleTime(graph, i)} aria-label="Remove time">✕</button>
                            {/if}
                          </div>
                        {/each}
                        <button type="button" disabled={editLocked} class="interactive text-[11px] text-muted-foreground hover:text-foreground" onclick={() => addScheduleTime(graph)}>+ Add time</button>
                      </div>
                    {/if}

                    <div class="space-y-1">
                      <span class="text-[11px] text-muted-foreground">Timezone</span>
                      <Input disabled={editLocked} value={s.tz} placeholder="America/Sao_Paulo" class="h-8" onchange={(e) => patchSchedule(graph, { tz: e.currentTarget.value.trim() })} />
                    </div>
                    <p class="text-[11px] text-muted-foreground">{scheduleSummary(s)}</p>
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

            <!-- Instance scope (#95): what this graph runs one of. Per-camera lets two cameras
                 record + stop independently instead of colliding on one shared window. -->
            <div class="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-3">
              <Label class="pt-2 text-xs text-muted-foreground">Instances</Label>
              <div class="space-y-1.5">
                <Select.Root
                  type="single"
                  disabled={editLocked}
                  value={instanceScopeKind(graph)}
                  onValueChange={(v) => applyInstanceScope(graph, v ?? "single")}
                >
                  <Select.Trigger class="h-8 w-full">{labelOf(INSTANCE_OPTS, instanceScopeKind(graph))}</Select.Trigger>
                  <Select.Content>
                    {#each INSTANCE_OPTS as o (o.value)}
                      <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
                <p class="text-[11px] leading-snug text-muted-foreground">{instanceScopeHint(graph)}</p>
                <button
                  class="inline-flex items-center gap-1 text-[11px] text-primary underline-offset-2 hover:underline"
                  onclick={() => goto(`/workflows/instances?graph=${graph.id}&name=${encodeURIComponent(graph.name)}`)}
                >
                  <Layers class="size-3" /> View live instances
                </button>
              </div>
            </div>
          </div>
          </div>
          {/if}

          <!-- ── Properties tab (hidden, not unmounted — see above) ──────────── -->
          <div
            class="flex h-full flex-col gap-3 overflow-y-auto"
            class:hidden={panelTab !== "properties"}
          >
          {#if selectedNode}
            {@const nk = catalog[selectedNode.kind]}
            <div class="space-y-2.5 rounded-xl border border-border bg-card p-3">
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
              {#if selectedNode.kind === "dict_schema"}
                <!-- Dictionary Schema (#113): define the columns (the plan). These become the
                     inputs of a Create Dictionary Row wired to this schema. -->
                <div class="space-y-1.5">
                  <div class="flex items-center justify-between">
                    <Label class="text-xs text-muted-foreground">Columns</Label>
                    <Button variant="ghost" size="icon" class="size-6" title="Add column" disabled={editLocked} onclick={() => addSchemaColumn(selectedNode)}>
                      <Plus class="size-3.5" />
                    </Button>
                  </div>
                  {#each schemaCols(selectedNode) as col, i (i)}
                    <div class="flex items-center gap-1.5">
                      <Input
                        class="h-7 min-w-0 flex-1 text-xs"
                        placeholder="name"
                        value={col.name ?? ""}
                        disabled={editLocked}
                        onchange={(e) => editSchemaColumn(selectedNode, i, { name: e.currentTarget.value.trim() })}
                      />
                      <select
                        class="h-7 w-[92px] shrink-0 rounded border border-input bg-background px-1 text-xs disabled:opacity-50"
                        value={col.type ?? "text"}
                        disabled={editLocked}
                        onchange={(e) => editSchemaColumn(selectedNode, i, { type: e.currentTarget.value })}
                      >
                        {#each CELL_TYPE_OPTS as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
                      </select>
                      <Button variant="ghost" size="icon" class="size-6 shrink-0 text-muted-foreground hover:text-destructive" title="Remove column" disabled={editLocked} onclick={() => removeSchemaColumn(selectedNode, i)}>
                        <Trash2 class="size-3" />
                      </Button>
                    </div>
                  {/each}
                  {#if schemaCols(selectedNode).length === 0}
                    <p class="text-[11px] leading-snug text-muted-foreground">Add columns (name + type) — they become the inputs of a Create Dictionary Row.</p>
                  {/if}
                </div>
              {:else if configRows.length === 0}
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
                      {:else if (key === "variable" || key === "type") && (selectedNode.kind === "get_variable" || selectedNode.kind === "set_variable")}
                        <!-- The variable's name + type are owned by the Variables panel; they're
                             baked onto the node and must not be edited here (editing would desync
                             the node from its declared variable + break the dynamic port type). -->
                        <Input class="h-8" value={String(selectedNode.config[key] ?? "")} disabled readonly aria-readonly="true" />
                      {:else if key === "tuya_device"}
                        {#if tuyaDeviceOptions.length === 0}
                          <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">
                            No Tuya devices — sync them in Settings → Integrations.
                          </p>
                        {:else}
                          {@const tv = String(selectedNode.config[key] ?? "")}
                          {@const tlabel = tuyaDeviceOptions.find((o) => o.value === tv)?.label ?? "Select a device"}
                          <Select.Root type="single" disabled={editLocked} value={tv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? tv, tuya_code: "" })}>
                            <Select.Trigger class="h-8 w-full">{tlabel}</Select.Trigger>
                            <Select.Content>
                              {#each tuyaDeviceOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if key === "tuya_code"}
                        {@const tdev = String(selectedNode.config["tuya_device"] ?? "")}
                        {@const copts = tuyaCodeOptions(tdev)}
                        {#if copts.length === 0}
                          <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">
                            Pick a device first, or re-sync to load its controls.
                          </p>
                        {:else}
                          {@const cvv = String(selectedNode.config[key] ?? "")}
                          {@const clbl = copts.find((o) => o.value === cvv)?.label ?? "Select a control"}
                          <Select.Root type="single" disabled={editLocked} value={cvv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? cvv, value_type: tuyaValueType(tuyaFunction(tdev, v ?? cvv)) })}>
                            <Select.Trigger class="h-8 w-full">{clbl}</Select.Trigger>
                            <Select.Content>
                              {#each copts as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if key === "value" && selectedNode.kind === "tuya_control"}
                        {@const tdev = String(selectedNode.config["tuya_device"] ?? "")}
                        {@const tfn = tuyaFunction(tdev, String(selectedNode.config["tuya_code"] ?? ""))}
                        {#if tfn?.type === "Boolean"}
                          <div class="flex items-center gap-2">
                            <Switch
                              disabled={editLocked}
                              checked={selectedNode.config[key] === true || selectedNode.config[key] === "true"}
                              onCheckedChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v })}
                            />
                            <span class="text-xs text-muted-foreground">{selectedNode.config[key] === true || selectedNode.config[key] === "true" ? "On" : "Off"}</span>
                          </div>
                        {:else if tfn?.type === "Enum"}
                          {@const range = tuyaEnumRange(tfn)}
                          {@const ev = String(selectedNode.config[key] ?? "")}
                          <Select.Root type="single" disabled={editLocked} value={ev} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? ev })}>
                            <Select.Trigger class="h-8 w-full">{ev || "Select a value"}</Select.Trigger>
                            <Select.Content>
                              {#each range as r (r)}<Select.Item value={r} label={r}>{r}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {:else if tfn?.type === "Integer"}
                          <Input type="number" class="h-8" disabled={editLocked} value={String(selectedNode.config[key] ?? "")} oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)} />
                        {:else}
                          <Input class="h-8" disabled={editLocked} value={String(selectedNode.config[key] ?? "")} oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)} />
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
                      {:else if key === "column"}
                        <!-- Dictionary column picker (#113): the node's `table` selects the dictionary,
                             then this lists its columns WITH their type — no more typing names blind. -->
                        {@const tid = String(selectedNode.config.table ?? "")}
                        {@const cols = columnOptions(tid)}
                        {#if cols.length === 0}
                          <Input
                            class="h-8"
                            disabled={editLocked}
                            placeholder="column name"
                            value={String(selectedNode.config[key] ?? "")}
                            onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })}
                          />
                          {#if !tid}<p class="mt-1 text-[10px] leading-snug text-muted-foreground">Pick this node's dictionary above to choose a column.</p>{/if}
                        {:else}
                          {@const cvc = String(selectedNode.config[key] ?? "")}
                          <Select.Root type="single" disabled={editLocked} value={cvc} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? cvc })}>
                            <Select.Trigger class="h-8 w-full">{cols.find((o) => o.value === cvc)?.label ?? "Pick a column"}</Select.Trigger>
                            <Select.Content>
                              {#each cols as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                            </Select.Content>
                          </Select.Root>
                        {/if}
                      {:else if port?.type === "table"}
                        {#if dictionaryOptions.length === 0}
                          <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">
                            No dictionaries — <button class="underline" onclick={() => goto("/workflows/dictionaries")}>create one</button>.
                          </p>
                        {:else}
                          {@const tv = resolveOpt(dictionaryOptions, String(selectedNode.config[key] ?? ""))}
                          <Select.Root type="single" disabled={editLocked} value={tv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? tv })}>
                            <Select.Trigger class="h-8 w-full">{dictionaryOptions.find((o) => o.value === tv)?.label ?? "Pick a dictionary"}</Select.Trigger>
                            <Select.Content>
                              {#each dictionaryOptions as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
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
                      {:else if key === "class" && (selectedNode.kind === "object_detection" || selectedNode.kind === "on_event")}
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
                      {:else if key === "zone_trigger" && selectedNode.kind === "on_event"}
                        {@const ZONE_TRIGGERS = [
                          { value: "intersects", label: "Intersects (looser)" },
                          { value: "centroid_inside", label: "Centre inside (stricter)" },
                        ]}
                        {@const ztv = String(selectedNode.config[key] ?? "intersects")}
                        {@const ztlabel = ZONE_TRIGGERS.find((o) => o.value === ztv)?.label ?? ztv}
                        <Select.Root type="single" disabled={editLocked} value={ztv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "intersects" })}>
                          <Select.Trigger class="h-8 w-full">{ztlabel}</Select.Trigger>
                          <Select.Content>
                            {#each ZONE_TRIGGERS as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                        <p class="mt-0.5 text-[10px] text-muted-foreground">Only matters if a Region (3+ points) is wired in below</p>
                      {:else if key in TRACKING_TUNING_DEFAULTS && selectedNode.kind === "on_event"}
                        <Input
                          type="number"
                          step={key === "min_hits" || key === "max_age" ? "1" : "0.05"}
                          min="0"
                          class="h-8"
                          disabled={editLocked}
                          value={String(selectedNode.config[key] ?? TRACKING_TUNING_DEFAULTS[key])}
                          oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)}
                        />
                        <p class="mt-0.5 text-[10px] text-muted-foreground">{TRACKING_TUNING_HELP[key]}</p>
                      {:else if key === "points" && selectedNode.kind === "region"}
                        {@const regionCameraIp = resolvedCameraIp(String(selectedNode.config.camera ?? ""))}
                        {@const regionPoints = (Array.isArray(selectedNode.config.points) ? selectedNode.config.points : []) as [number, number][]}
                        <RegionPointsEditor
                          cameraIp={regionCameraIp}
                          points={regionPoints}
                          disabled={editLocked}
                          onchange={(next) => saveNodeConfig(selectedNode, { ...selectedNode.config, points: next })}
                        />
                      {:else if key === "type" && selectedNode.kind === "ai_extract"}
                        {@const EXTRACT_TYPES = [
                          { value: "text", label: "Text" },
                          { value: "camera", label: "Camera" },
                          { value: "recipient", label: "Recipient" },
                          { value: "num", label: "Number" },
                        ]}
                        {@const etv = String(selectedNode.config[key] ?? "text")}
                        {@const etlabel = EXTRACT_TYPES.find((o) => o.value === etv)?.label ?? etv}
                        <Select.Root type="single" disabled={editLocked} value={etv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "text" })}>
                          <Select.Trigger class="h-8 w-full">{etlabel}</Select.Trigger>
                          <Select.Content>
                            {#each EXTRACT_TYPES as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
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
                        <!-- Event name persists like any other field (→ chooseEvent → saveNodeConfig).
                             Registration into the tenant registry (so other graphs can listen + it
                             appears in the palette under its category) is an explicit step.
                             A real Select, NOT a <datalist>: datalist never opens on iPad/iOS Safari
                             and is inconsistent on Android, so on a tablet the picker looked like an
                             empty text box with no hint that any event existed. The free-text input
                             below stays as the escape hatch for a brand-new custom event name. -->
                        {@const evName = String(selectedNode.config[key] ?? "").trim()}
                        {@const known = events.find((e) => e.name === evName)}
                        <Select.Root
                          type="single"
                          disabled={editLocked}
                          value={known ? evName : ""}
                          onValueChange={(v) => { if (v) chooseEvent(selectedNode, v); }}
                        >
                          <Select.Trigger class="h-8 w-full">
                            {known ? evName : evName || "Choose an event…"}
                          </Select.Trigger>
                          <Select.Content>
                            {#each events as e (e.name)}
                              <Select.Item value={e.name} label={e.name}>{e.name} · {e.category}</Select.Item>
                            {/each}
                          </Select.Content>
                        </Select.Root>
                        {#if known}
                          <p class="mt-1 text-[10px] text-muted-foreground">Registered · “{known.category}”</p>
                        {:else}
                          <Input
                            class="mt-1.5 h-8"
                            disabled={editLocked}
                            placeholder="…or type a new event name"
                            value={String(selectedNode.config[key] ?? "")}
                            onchange={(e) => chooseEvent(selectedNode, e.currentTarget.value)}
                          />
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
                      {:else if key === "expand"}
                        {@const ev = Number(selectedNode.config[key] ?? 0)}
                        <div class="flex items-center gap-2">
                          <Slider
                            type="single"
                            min={0}
                            max={100}
                            step={1}
                            value={ev}
                            disabled={editLocked}
                            class="flex-1"
                            onValueChange={(v) => liveSaveConfig(selectedNode, key, String(v))}
                          />
                          <span class="w-9 text-right text-xs tabular-nums text-muted-foreground">{ev}%</span>
                        </div>
                      {:else if key === "color" || key === "text_color" || key === "bg_color"}
                        <input
                          type="color"
                          class="h-8 w-full cursor-pointer rounded-lg border border-border bg-card"
                          disabled={editLocked}
                          value={String(selectedNode.config[key] ?? COMMENT_DEFAULT_COLOR)}
                          oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)}
                          onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })}
                        />
                      {:else if key === "position" && selectedNode.kind === "draw_text"}
                        {@const POSITIONS = [
                          { value: "top-left", label: "Top-left" },
                          { value: "top-right", label: "Top-right" },
                          { value: "bottom-left", label: "Bottom-left" },
                          { value: "bottom-right", label: "Bottom-right" },
                        ]}
                        {@const pv = String(selectedNode.config[key] ?? "top-left")}
                        {@const plabel = POSITIONS.find((o) => o.value === pv)?.label ?? pv}
                        <Select.Root type="single" disabled={editLocked} value={pv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "top-left" })}>
                          <Select.Trigger class="h-8 w-full">{plabel}</Select.Trigger>
                          <Select.Content>
                            {#each POSITIONS as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else if key === "background"}
                        <div class="flex h-8 items-center gap-2">
                          <Switch
                            checked={selectedNode.config[key] === true || selectedNode.config[key] === "true"}
                            disabled={editLocked}
                            onCheckedChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v })}
                          />
                          <span class="text-xs text-muted-foreground">{selectedNode.config[key] === true || selectedNode.config[key] === "true" ? "On" : "Off"}</span>
                        </div>
                      {:else if key === "question"}
                        <Textarea
                          rows={4}
                          class="min-h-20 resize-y"
                          disabled={editLocked}
                          placeholder="Ask a yes/no question about the inputs…"
                          value={String(selectedNode.config[key] ?? "")}
                          oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)}
                        />
                      {:else if key === "size" && selectedNode.kind === "title"}
                        {@const SIZES = [{ value: "sm", label: "Small" }, { value: "md", label: "Medium" }, { value: "lg", label: "Large" }, { value: "xl", label: "Extra large" }]}
                        {@const sv = String(selectedNode.config[key] ?? "lg")}
                        <Select.Root type="single" disabled={editLocked} value={sv} onValueChange={(v) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: v ?? "lg" })}>
                          <Select.Trigger class="h-8 w-full">{SIZES.find((o) => o.value === sv)?.label ?? sv}</Select.Trigger>
                          <Select.Content>
                            {#each SIZES as o (o.value)}<Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>{/each}
                          </Select.Content>
                        </Select.Root>
                      {:else if key === "text" && (selectedNode.kind === "paragraph" || selectedNode.kind === "markdown")}
                        <Textarea
                          rows={6}
                          class="min-h-28 resize-y font-mono text-xs"
                          disabled={editLocked}
                          placeholder={selectedNode.kind === "markdown" ? "## Heading\n- point one" : "Description…"}
                          value={String(selectedNode.config[key] ?? "")}
                          oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)}
                        />
                      {:else if port?.type === "region"}
                        <p class="pt-1.5 text-xs text-muted-foreground">Wire a <span class="font-medium text-foreground">Region</span> node here for a line or zone — see Vision in the palette.</p>
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
              <!-- String / AI switch: user-defined arms. Each arm becomes an exec output port. A
                   built-in "default" arm fires when no arm matches (String) / no confident semantic
                   match (AI). Same arm-editing UX; AI Switch picks the arm by meaning. -->
              {#if selectedNode.kind === "string_switch" || selectedNode.kind === "ai_switch"}
                {@const swArms = Array.isArray(selectedNode.config?.arms) ? (selectedNode.config.arms as string[]) : []}
                {@const isAiSwitch = selectedNode.kind === "ai_switch"}
                <div class="space-y-2.5 border-t border-border pt-2.5">
                  <p class="text-xs font-medium text-muted-foreground">{isAiSwitch ? "Intent arms" : "Match arms"}</p>
                  <div class="space-y-1.5">
                    {#each swArms as arm, i (i)}
                      <div class="flex items-center gap-1.5">
                        <Input
                          class="h-8 min-w-0 flex-1"
                          disabled={editLocked}
                          placeholder={isAiSwitch ? "intent label (e.g. take a photo)" : "arm label (e.g. red)"}
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
                  <p class="text-[10px] text-muted-foreground">
                    {#if isAiSwitch}A model routes the message to the closest arm by meaning; a built-in <strong>default</strong> fires when none is a confident match.
                    {:else}A built-in <strong>default</strong> arm always fires when no label matches.{/if}
                  </p>
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
            <div class="rounded-xl border border-dashed border-border p-6 text-center">
              <OlharMark size={28} class="mx-auto mb-2 text-muted-foreground/50" label="" />
              <p class="text-xs text-muted-foreground">Select a node to edit its properties.</p>
            </div>
          {/if}
          </div>

          <!-- ── Variables tab ───────────────────────────────────────────────── -->
          {#if panelTab === "variables"}
            <div class="h-full overflow-y-auto">
              <VariablesPanel
                {variables}
                types={VAR_TYPES}
                cameras={cameraOptions}
                recipients={recipientOptions}
                dictionaries={dictionaryOptions}
                disabled={editLocked}
                onAdd={addVariable}
                onEdit={editVariable}
                onRemove={removeVariable}
                onNewDictionary={openNewDictionary}
                onEditDictionary={openEditDictionary}
              />
            </div>
          {/if}

          <!-- ── Runs tab — run history without leaving the editor ────────────── -->
          {#if panelTab === "runs"}
            <div class="flex h-full flex-col gap-2 overflow-y-auto rounded-xl border border-border bg-card p-2.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">Recent runs</span>
                <div class="flex items-center gap-0.5">
                  <Button variant="ghost" size="icon" class="size-6" title="Refresh" onclick={() => loadRuns()}>
                    <RefreshCw class="size-3.5 {runsLoading ? 'animate-spin' : ''}" />
                  </Button>
                  <a
                    href="/logs?module=workflow&entity_type=workflow_graph&entity_id={graph.id}"
                    class="rounded-md px-1.5 py-0.5 text-[10px] text-primary hover:underline"
                  >All logs →</a>
                </div>
              </div>

              {#if runsLoading && runsList.length === 0}
                <Loader size={28} />
              {:else if runsList.length === 0}
                <p class="rounded-lg border border-dashed border-border px-2 py-6 text-center text-[11px] text-muted-foreground">
                  No runs recorded yet. Run the graph and its history lands here.
                </p>
              {:else}
                <div class="space-y-1">
                  {#each runsList as r (r.id)}
                    <div class="rounded-md border border-border bg-elev-1">
                      <button
                        class="interactive flex w-full items-center gap-1.5 px-2 py-1.5 text-left"
                        onclick={() => toggleRunLogs(r.id)}
                      >
                        {#if runsExpanded === r.id}<ChevronDown class="size-3 shrink-0 text-muted-foreground" />{:else}<ChevronRight class="size-3 shrink-0 text-muted-foreground" />{/if}
                        <span class="inline-flex shrink-0 items-center rounded-full border px-1.5 py-0 text-[9px] font-medium capitalize {runStatusClass(r.status)}">
                          {r.status}
                        </span>
                        <span class="truncate text-[11px] font-medium">{triggerSourceLabel(r.trigger_source)}</span>
                        <span class="ml-auto shrink-0 text-[10px] text-muted-foreground">{fmtTs(r.started_at)}</span>
                      </button>
                      {#if runsExpanded === r.id}
                        <div class="border-t border-border px-2 py-1.5">
                          {#if r.result_summary}<p class="mb-1 text-[10px] text-muted-foreground">{r.result_summary}</p>{/if}
                          {#if (runsLogs[r.id]?.length ?? 0) === 0}
                            <p class="text-[10px] text-muted-foreground/60">No detailed signals recorded.</p>
                          {:else}
                            <div class="space-y-0.5 font-mono text-[10px] leading-relaxed">
                              {#each runsLogs[r.id] as line (line.id)}
                                <div class="flex gap-1.5">
                                  <span class="w-8 shrink-0 uppercase {line.level === 'error' ? 'text-destructive' : line.level === 'warn' ? 'text-amber-400' : 'text-muted-foreground/60'}">{line.level}</span>
                                  <span class="min-w-0 break-words {line.level === 'error' ? 'text-destructive' : 'text-muted-foreground'}">
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
                </div>
              {/if}
            </div>
          {/if}

          </div>
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
      <div class="shrink-0 overflow-hidden rounded-xl border border-border bg-card">
        <button
          class="flex w-full items-center justify-between px-3 py-2 text-sm transition-colors hover:bg-hover"
          onclick={toggleConsole}
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

<!-- Run inputs prompt (#102): fill in the graph's input variables before running -->
<Dialog.Root bind:open={runPromptOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Run {runPromptGraph?.name ?? "workflow"}</Dialog.Title>
      <Dialog.Description>Fill in the inputs for this run. They're pre-filled with the defaults.</Dialog.Description>
    </Dialog.Header>
    <div class="space-y-3 py-1">
      {#each runParams as v (v.name)}
        <div class="space-y-1.5">
          <label class="text-xs font-medium" for="run-in-{v.name}">
            {v.name}
            {#if v.description}<span class="ml-1 font-normal text-muted-foreground">— {v.description}</span>{/if}
          </label>
          {#if v.type === "bool"}
            <select id="run-in-{v.name}" class="h-8 w-full rounded border border-input bg-background px-2 text-sm" bind:value={runInputs[v.name]}>
              <option value="true">true</option><option value="false">false</option>
            </select>
          {:else if v.type === "camera"}
            <select id="run-in-{v.name}" class="h-8 w-full rounded border border-input bg-background px-2 text-sm" bind:value={runInputs[v.name]}>
              {#each cameraOptions as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
            </select>
          {:else if v.type === "recipient"}
            <select id="run-in-{v.name}" class="h-8 w-full rounded border border-input bg-background px-2 text-sm" bind:value={runInputs[v.name]}>
              {#each recipientOptions as r (r.value)}<option value={r.value}>{r.label}</option>{/each}
            </select>
          {:else}
            <Input id="run-in-{v.name}" type={v.type === "num" ? "number" : "text"} bind:value={runInputs[v.name]} />
          {/if}
        </div>
      {/each}
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (runPromptOpen = false)}>Cancel</Button>
      <Button onclick={confirmRun} disabled={running}>Run</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Dictionary create/edit modal (from a Table variable row) -->
<DictionaryModal
  bind:open={dictModalOpen}
  dictId={dictModalId}
  cameras={cameraOptions}
  recipients={recipientOptions}
  onsaved={onDictionarySaved}
/>

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
  kinds={menuKinds}
  onSelect={addFromContext}
/>

<!-- Drop chooser: after dragging a variable onto the canvas, pick Get or Set at the drop point. -->
{#if varDrop}
  <button class="fixed inset-0 z-[59]" aria-label="Cancel" onclick={() => (varDrop = null)}></button>
  <div
    class="fixed z-[60] flex flex-col overflow-hidden rounded-xl border border-border bg-popover/95 shadow-xl backdrop-blur-sm duration-150 animate-in fade-in zoom-in-95"
    style:left="{Math.min(varDrop.screen.x, (typeof window !== 'undefined' ? window.innerWidth : 9999) - 180)}px"
    style:top="{Math.min(varDrop.screen.y, (typeof window !== 'undefined' ? window.innerHeight : 9999) - 110)}px"
    style:width="170px"
  >
    <div class="truncate border-b border-border px-3 py-1.5 text-[11px] font-medium text-muted-foreground">{varDrop.v.name}</div>
    <button class="interactive flex items-center gap-2 px-3 py-2 text-left text-xs hover:bg-accent" onclick={() => chooseVarNode("get_variable")}>
      <span class="size-1.5 rounded-full bg-primary"></span> Get {varDrop.v.name}
    </button>
    <button class="interactive flex items-center gap-2 px-3 py-2 text-left text-xs hover:bg-accent" onclick={() => chooseVarNode("set_variable")}>
      <span class="size-1.5 rounded-full bg-sky-400"></span> Set {varDrop.v.name}
    </button>
  </div>
{/if}

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
          <div class="rounded-lg border border-border bg-card">
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
  /* Node body fill — the "raised" stop of the elevation ramp, so nodes read as
     panels lifted off the recessed canvas. */
  :global(.node-body) {
    fill: var(--wf-node-body);
  }

  /* Tactile ports: a subtle grow + halo on hover so pins feel clickable. */
  :global(.wf-port) {
    transform-box: fill-box;
    transform-origin: center;
    transition:
      transform 0.12s ease-out,
      filter 0.12s ease-out;
  }
  @media (prefers-reduced-motion: no-preference) {
    :global(.wf-port:hover) {
      transform: scale(1.28);
      filter: drop-shadow(0 0 3px currentColor);
    }
  }
</style>
