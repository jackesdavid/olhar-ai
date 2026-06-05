<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { toast } from "svelte-sonner";

  import Plus from "@lucide/svelte/icons/plus";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import Square from "@lucide/svelte/icons/square";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Power from "@lucide/svelte/icons/power";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Hand from "@lucide/svelte/icons/hand";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import * as Dialog from "$lib/components/ui/dialog";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  // ---- types (the serialized core shapes) ----------------------------------
  type PortType = "text" | "num" | "bool" | "image";
  type CatPort = { name: string; type: PortType; required: boolean };
  type CatKind = {
    slug: string;
    label: string;
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
    nodes: Node[];
    edges: Edge[];
  };

  // ---- layout constants ----------------------------------------------------
  const NODE_W = 176;
  const HEAD_H = 26;
  const ROW_H = 22;
  const PAD = 8;

  // ---- state ---------------------------------------------------------------
  let graphs = $state<Graph[]>([]);
  let catalog = $state<Record<string, CatKind>>({});
  let selectedGraphId = $state<number | null>(null);
  let selectedNodeId = $state<number | null>(null);
  let pending = $state<{ node: number; port: string } | null>(null);
  let loading = $state(true);
  let running = $state(false);
  let addOpen = $state(false);

  // ---- dialogs (window.prompt/confirm are no-ops in the Tauri WebView) ------
  let nameOpen = $state(false);
  let nameValue = $state("");
  let nameTarget = $state<Graph | null>(null); // null = create, else rename
  let deleteOpen = $state(false);
  let deleteTarget = $state<Graph | null>(null);

  const graph = $derived(graphs.find((g) => g.id === selectedGraphId) ?? null);
  const selectedNode = $derived(graph?.nodes.find((n) => n.id === selectedNodeId) ?? null);
  const kinds = $derived(Object.values(catalog));

  function nodeHeight(n: Node): number {
    const k = catalog[n.kind];
    const rows = Math.max(k?.inputs.length ?? 0, k?.outputs.length ?? 0, 1);
    return HEAD_H + rows * ROW_H + PAD;
  }
  function inPortY(n: Node, i: number) {
    return n.y + HEAD_H + i * ROW_H + ROW_H / 2;
  }
  function outPortY(n: Node, i: number) {
    return n.y + HEAD_H + i * ROW_H + ROW_H / 2;
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
  };

  // ---- load ----------------------------------------------------------------
  async function reload() {
    graphs = await invoke<Graph[]>("load_graphs");
    if (selectedGraphId === null && graphs.length > 0) selectedGraphId = graphs[0].id;
    if (!graphs.some((g) => g.id === selectedGraphId)) selectedGraphId = graphs[0]?.id ?? null;
  }

  onMount(() => {
    (async () => {
      try {
        const cat = await invoke<{ kinds: CatKind[] }>("node_catalog");
        catalog = Object.fromEntries(cat.kinds.map((k) => [k.slug, k]));
        await reload();
      } catch (err) {
        toast.error("Failed to load workflows", { description: String(err) });
      } finally {
        loading = false;
      }
    })();
    const un = listen<{ name: string; ok: boolean; summary: string }>("workflow-run", (e) => {
      running = false;
      const { name, ok, summary } = e.payload;
      if (ok) toast.success(`Workflow "${name}"`, { description: summary });
      else toast.error(`Workflow "${name}" failed`, { description: summary });
    });
    return () => un.then((u) => u());
  });

  // ---- graph CRUD ----------------------------------------------------------
  function openCreate() {
    nameTarget = null;
    nameValue = "Untitled";
    nameOpen = true;
  }
  function openRename(g: Graph) {
    nameTarget = g;
    nameValue = g.name;
    nameOpen = true;
  }
  async function submitName() {
    const name = nameValue.trim();
    if (!name) {
      toast.error("Name can't be empty.");
      return;
    }
    try {
      if (nameTarget) {
        await invoke("rename_graph", { id: nameTarget.id, name });
      } else {
        const id = await invoke<number>("create_graph", { name });
        selectedGraphId = id;
        selectedNodeId = null;
      }
      nameOpen = false;
      await reload();
    } catch (err) {
      toast.error("Failed", { description: String(err) });
    }
  }
  function openDelete(g: Graph) {
    deleteTarget = g;
    deleteOpen = true;
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      await invoke("delete_graph", { id: deleteTarget.id });
      if (selectedGraphId === deleteTarget.id) selectedGraphId = null;
      deleteOpen = false;
      deleteTarget = null;
      await reload();
    } catch (err) {
      toast.error("Failed to delete", { description: String(err) });
    }
  }
  async function toggleEnabled(g: Graph, enabled: boolean) {
    await invoke("set_graph_enabled", { id: g.id, enabled });
    await reload();
  }
  async function runNow(g: Graph) {
    running = true;
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
    if (!graph) return;
    const n = graph.nodes.length;
    const x = 80 + (n % 5) * 48;
    const y = 70 + (n % 5) * 40;
    const id = await invoke<number>("add_node", { graphId: graph.id, kind: slug, x, y });
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
  function clickOutPort(node: Node, port: string) {
    pending = { node: node.id, port };
  }
  async function clickInPort(node: Node, port: string) {
    if (!graph) return;
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

  // ---- node dragging -------------------------------------------------------
  let drag: { id: number; offX: number; offY: number } | null = null;
  function startDrag(e: PointerEvent, n: Node) {
    selectedNodeId = n.id;
    drag = { id: n.id, offX: 0, offY: 0 };
    const svg = (e.currentTarget as Element).closest("svg") as SVGSVGElement | null;
    if (svg) {
      const pt = svgPoint(svg, e);
      drag = { id: n.id, offX: pt.x - n.x, offY: pt.y - n.y };
    }
  }
  function onDragMove(e: PointerEvent) {
    if (!drag || !graph) return;
    const svg = e.currentTarget as SVGSVGElement;
    const pt = svgPoint(svg, e);
    const n = graph.nodes.find((x) => x.id === drag!.id);
    if (n) {
      n.x = Math.max(0, Math.round(pt.x - drag.offX));
      n.y = Math.max(0, Math.round(pt.y - drag.offY));
    }
  }
  async function endDrag() {
    if (!drag || !graph) return;
    const id = drag.id;
    const n = graph.nodes.find((x) => x.id === id);
    drag = null;
    if (n) await invoke("move_node", { graphId: graph.id, nodeId: id, x: n.x, y: n.y });
  }
  function svgPoint(svg: SVGSVGElement, e: PointerEvent) {
    const r = svg.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  const LEVELS = ["debug", "info", "warn", "error"];
</script>

<div class="relative h-[calc(100vh-3.5rem)] w-full">
  {#if loading}
    <div class="flex h-full items-center justify-center">
      <Loader label="Loading workflows…" />
    </div>
  {:else if graphs.length === 0}
    <div class="flex h-full items-center justify-center">
      <EmptyState title="No workflows yet" description="Create one to start automating.">
        <Button onclick={openCreate}><Plus class="size-4" /> New workflow</Button>
      </EmptyState>
    </div>
  {:else}
    <!-- Full-bleed node editor -->
    <div
      class="absolute inset-0 overflow-auto rounded-xl border border-border bg-card/20 bg-[radial-gradient(circle,_rgba(52,211,153,0.1)_1px,_transparent_1px)] [background-size:18px_18px]"
    >
      {#if graph}
        <svg
          width="1600"
          height="900"
          class="block"
          onpointermove={onDragMove}
          onpointerup={endDrag}
          onpointerleave={endDrag}
          role="presentation"
        >
          <defs>
            <filter id="nodeGlow" x="-40%" y="-40%" width="180%" height="180%">
              <feDropShadow dx="0" dy="0" stdDeviation="5" flood-color="#10b981" flood-opacity="0.5" />
            </filter>
          </defs>

          <!-- edges -->
          {#each graph.edges as e, idx (idx)}
            {@const fromN = graph.nodes.find((n) => n.id === e.from)}
            {@const toN = graph.nodes.find((n) => n.id === e.to)}
            {#if fromN && toN}
              {@const x1 = fromN.x + NODE_W}
              {@const y1 = outPortY(fromN, Math.max(0, portIndex(fromN, e.from_port, "out")))}
              {@const x2 = toN.x}
              {@const y2 = inPortY(toN, Math.max(0, portIndex(toN, e.to_port, "in")))}
              <path
                d={`M ${x1} ${y1} C ${x1 + 60} ${y1}, ${x2 - 60} ${y2}, ${x2} ${y2}`}
                fill="none"
                stroke-width="2"
                class="cursor-pointer stroke-primary/40 transition-colors hover:stroke-destructive"
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
            <g filter={sel ? "url(#nodeGlow)" : undefined}>
              <rect
                x={n.x}
                y={n.y}
                width={NODE_W}
                height={h}
                rx="8"
                stroke-width={sel ? 2 : 1}
                class="fill-card transition-[stroke] {sel
                  ? 'stroke-primary'
                  : 'stroke-border hover:stroke-primary/50'}"
              />
              <!-- title row: emerald accent dot + label + transparent drag handle -->
              <rect
                x={n.x}
                y={n.y}
                width={NODE_W}
                height={HEAD_H}
                fill="transparent"
                class="cursor-move"
                onpointerdown={(ev) => startDrag(ev, n)}
                role="presentation"
              />
              <circle
                cx={n.x + 12}
                cy={n.y + HEAD_H / 2}
                r="2.5"
                class="pointer-events-none fill-primary"
              />
              <text
                x={n.x + 20}
                y={n.y + 17}
                class="pointer-events-none select-none fill-foreground"
                font-size="11"
                font-weight="600"
              >
                {k?.label ?? n.kind}
              </text>
              <line
                x1={n.x}
                y1={n.y + HEAD_H}
                x2={n.x + NODE_W}
                y2={n.y + HEAD_H}
                class="pointer-events-none stroke-border"
                stroke-width="1"
              />

              <!-- input ports -->
              {#each k?.inputs ?? [] as p, i}
                {@const wired = wiredInto(n.id, p.name)}
                <circle
                  cx={n.x}
                  cy={inPortY(n, i)}
                  r="5"
                  fill={wired ? portColor[p.type] : "#151c19"}
                  stroke={portColor[p.type]}
                  stroke-width="2"
                  class="cursor-pointer"
                  onclick={() => clickInPort(n, p.name)}
                  role="presentation"
                />
                <text
                  x={n.x + 11}
                  y={inPortY(n, i) + 3}
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
                  cx={n.x + NODE_W}
                  cy={outPortY(n, i)}
                  r={isPending ? 6 : 5}
                  fill={isPending ? portColor[p.type] : "#151c19"}
                  stroke={portColor[p.type]}
                  stroke-width="2"
                  class="cursor-pointer transition-all"
                  onclick={() => clickOutPort(n, p.name)}
                  role="presentation"
                />
                <text
                  x={n.x + NODE_W - 11}
                  y={outPortY(n, i) + 3}
                  text-anchor="end"
                  class="pointer-events-none select-none fill-muted-foreground"
                  font-size="10"
                >
                  {p.name}
                </text>
              {/each}
            </g>
          {/each}
        </svg>
      {:else}
        <div class="flex h-full items-center justify-center">
          <EmptyState
            title="No workflow selected"
            description="Pick a workflow top-left, or create one to start wiring nodes."
          />
        </div>
      {/if}
    </div>

    {#if graph}
      <!-- TOP-LEFT: workflow switcher + add node -->
      <div class="absolute left-3 top-3 flex items-center gap-2">
        <div
          class="flex items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
        >
          <div class="relative">
            <select
              class="h-8 appearance-none rounded-md bg-transparent pl-2.5 pr-7 text-sm font-medium outline-none"
              value={String(selectedGraphId)}
              onchange={(e) => {
                selectedGraphId = Number(e.currentTarget.value);
                selectedNodeId = null;
                pending = null;
              }}
            >
              {#each graphs as g (g.id)}<option value={g.id}>{g.name}</option>{/each}
            </select>
            <ChevronDown
              class="pointer-events-none absolute right-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            />
          </div>
          <Button variant="ghost" size="icon" class="size-8" title="New workflow" onclick={openCreate}>
            <Plus class="size-4" />
          </Button>
        </div>
        <Button class="shadow-lg" onclick={() => (addOpen = true)}>
          <Plus class="size-4" /> Add node
        </Button>
      </div>

      <!-- TOP-CENTER: transport -->
      <div
        class="absolute left-1/2 top-3 flex -translate-x-1/2 items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
      >
        <Button
          variant="ghost"
          size="icon"
          class="size-8 text-primary hover:text-primary"
          title="Run now"
          disabled={running}
          onclick={() => runNow(graph)}
        >
          {#if running}<LoaderCircle class="size-4 animate-spin" />{:else}<Play class="size-4" />{/if}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          class="size-8"
          title="Pause (coming soon)"
          onclick={() => comingSoon("Pause")}
        >
          <Pause class="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          class="size-8"
          title="Stop (coming soon)"
          onclick={() => comingSoon("Stop")}
        >
          <Square class="size-4" />
        </Button>
      </div>

      <!-- RIGHT COLUMN: zoom · global properties (top) · node property (middle) -->
      <div class="pointer-events-none absolute bottom-3 right-3 top-3 flex w-72 flex-col gap-2">
        <!-- zoom / pan -->
        <div
          class="pointer-events-auto ml-auto flex items-center gap-1 rounded-lg border border-border bg-background/75 p-1 shadow-lg backdrop-blur"
        >
          <Button variant="ghost" size="icon" class="size-8" title="Zoom in (coming soon)" onclick={() => comingSoon("Zoom")}>
            <ZoomIn class="size-4" />
          </Button>
          <Button variant="ghost" size="icon" class="size-8" title="Zoom out (coming soon)" onclick={() => comingSoon("Zoom")}>
            <ZoomOut class="size-4" />
          </Button>
          <Button variant="ghost" size="icon" class="size-8" title="Fit to view (coming soon)" onclick={() => comingSoon("Fit to view")}>
            <Maximize class="size-4" />
          </Button>
          <Button variant="ghost" size="icon" class="size-8" title="Pan (coming soon)" onclick={() => comingSoon("Pan")}>
            <Hand class="size-4" />
          </Button>
        </div>

        <!-- global properties -->
        <div
          class="pointer-events-auto space-y-3 rounded-xl border border-border bg-background/85 p-3 shadow-lg backdrop-blur"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="flex min-w-0 items-center gap-2 text-sm font-medium">
              <StatusDot state={graph.enabled ? "online" : "offline"} />
              <span class="truncate">{graph.name}</span>
            </span>
            <div class="flex shrink-0">
              <Button variant="ghost" size="icon" class="size-7" title="Rename" onclick={() => openRename(graph)}>
                <Pencil class="size-3.5" />
              </Button>
              <Button variant="ghost" size="icon" class="size-7" title="Delete" onclick={() => openDelete(graph)}>
                <Trash2 class="size-3.5" />
              </Button>
            </div>
          </div>

          <div class="flex items-center justify-between">
            <Label class="flex items-center gap-1.5 text-sm"><Power class="size-3.5" /> Enabled</Label>
            <Switch checked={graph.enabled} onCheckedChange={(v) => toggleEnabled(graph, v)} />
          </div>

          <div class="space-y-1.5">
            <Label class="text-xs text-muted-foreground">Trigger</Label>
            <select
              class="h-9 w-full rounded-md border border-input bg-transparent px-2 text-sm"
              value={triggerKind(graph.trigger)}
              onchange={(e) => applyTrigger(graph, e.currentTarget.value, triggerSecs(graph.trigger), triggerLevel(graph.trigger))}
            >
              <option value="manual">Manual (Run-now only)</option>
              <option value="cron">Cron (every N seconds)</option>
              <option value="on_new_alert">On new alert</option>
            </select>
            {#if triggerKind(graph.trigger) === "cron"}
              <div class="flex items-center gap-2">
                <Input
                  type="number"
                  min="1"
                  value={triggerSecs(graph.trigger)}
                  class="h-8"
                  onchange={(e) => applyTrigger(graph, "cron", Number(e.currentTarget.value), "info")}
                />
                <span class="text-xs text-muted-foreground">sec</span>
              </div>
            {:else if triggerKind(graph.trigger) === "on_new_alert"}
              <select
                class="h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                value={triggerLevel(graph.trigger)}
                onchange={(e) => applyTrigger(graph, "on_new_alert", 60, e.currentTarget.value)}
              >
                {#each LEVELS as l}<option value={l}>≥ {l}</option>{/each}
              </select>
            {/if}
          </div>

          <div class="space-y-1.5">
            <Label class="text-xs text-muted-foreground">Concurrency</Label>
            <select
              class="h-9 w-full rounded-md border border-input bg-transparent px-2 text-sm"
              value={concKind(graph.concurrency)}
              onchange={(e) => applyConcurrency(graph, e.currentTarget.value, concMax(graph.concurrency))}
            >
              <option value="skip">Skip if running</option>
              <option value="queue">Queue (max N)</option>
              <option value="concurrent">Concurrent (max N)</option>
            </select>
            {#if concKind(graph.concurrency) !== "skip"}
              <Input
                type="number"
                min="1"
                value={concMax(graph.concurrency)}
                class="h-8"
                onchange={(e) => applyConcurrency(graph, concKind(graph.concurrency), Number(e.currentTarget.value))}
              />
            {/if}
          </div>
        </div>

        <!-- node property (centered in the remaining space) -->
        {#if selectedNode}
          {@const nk = catalog[selectedNode.kind]}
          <div class="flex-1"></div>
          <div
            class="pointer-events-auto max-h-[55%] space-y-2.5 overflow-y-auto rounded-xl border border-border bg-background/85 p-3 shadow-lg backdrop-blur"
          >
            <div class="flex items-center justify-between gap-2">
              <span class="flex min-w-0 items-center gap-2 text-sm font-medium">
                <span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
                <span class="truncate">{nk?.label ?? selectedNode.kind}</span>
                <span class="shrink-0 text-muted-foreground">#{selectedNode.id}</span>
              </span>
              <Button
                variant="ghost"
                size="icon"
                class="size-7 shrink-0"
                title="Delete node"
                onclick={() => removeNode(selectedNode)}
              >
                <Trash2 class="size-3.5" />
              </Button>
            </div>
            {#if Object.keys(selectedNode.config).length === 0}
              <p class="text-xs text-muted-foreground">This node has no settings.</p>
            {:else}
              <div class="space-y-2.5">
                {#each Object.keys(selectedNode.config) as key (key)}
                  {@const wired = wiredInto(selectedNode.id, key)}
                  <div class="space-y-1">
                    <Label class="text-xs text-muted-foreground">{key}</Label>
                    {#if wired}
                      <p
                        class="rounded border border-dashed border-primary/30 bg-primary/5 px-2 py-1.5 text-xs text-primary/80"
                      >
                        ← #{wired.from}.{wired.from_port}
                      </p>
                    {:else if key === "level"}
                      <select
                        class="h-8 w-full rounded-md border border-input bg-transparent px-2 text-sm"
                        value={String(selectedNode.config[key] ?? "info")}
                        onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })}
                      >
                        {#each LEVELS as l}<option value={l}>{l}</option>{/each}
                      </select>
                    {:else}
                      <Input
                        class="h-8"
                        value={String(selectedNode.config[key] ?? "")}
                        onchange={(e) => saveNodeConfig(selectedNode, { ...selectedNode.config, [key]: e.currentTarget.value })}
                      />
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
          <div class="flex-1"></div>
        {/if}
      </div>

      <!-- BOTTOM-CENTER: wiring hint / pending -->
      <div class="absolute bottom-3 left-1/2 -translate-x-1/2">
        {#if pending}
          <div
            class="flex items-center gap-2 rounded-full border border-primary/40 bg-background/85 px-3 py-1 text-xs text-primary shadow-lg backdrop-blur"
          >
            wiring from #{pending.node}.{pending.port} — click an input port
            <button
              class="text-muted-foreground transition-colors hover:text-foreground"
              onclick={() => (pending = null)}
            >
              cancel
            </button>
          </div>
        {:else}
          <div
            class="rounded-full border border-border bg-background/70 px-3 py-1 text-[11px] text-muted-foreground shadow-lg backdrop-blur"
          >
            Click an output ●, then an input ○ to wire · click a wired input to unwire
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<!-- Create / rename dialog -->
<Dialog.Root bind:open={nameOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{nameTarget ? "Rename workflow" : "New workflow"}</Dialog.Title>
    </Dialog.Header>
    <div class="space-y-2">
      <Label for="wf-name">Name</Label>
      <Input
        id="wf-name"
        bind:value={nameValue}
        placeholder="e.g. Night patrol"
        onkeydown={(e) => e.key === "Enter" && submitName()}
      />
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (nameOpen = false)}>Cancel</Button>
      <Button onclick={submitName}>{nameTarget ? "Save" : "Create"}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

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
      class="relative z-10 w-full max-w-lg rounded-2xl border border-border bg-popover/95 p-4 shadow-2xl duration-200 animate-in fade-in zoom-in-95"
    >
      <div class="mb-3 flex items-center justify-between">
        <h3 class="flex items-center gap-2 text-sm font-semibold">
          <span class="size-1.5 rounded-full bg-primary"></span> Add a node
        </h3>
        <Button variant="ghost" size="icon" class="size-7" onclick={() => (addOpen = false)}>
          <X class="size-4" />
        </Button>
      </div>
      <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {#each kinds as k (k.slug)}
          <button
            class="interactive group flex flex-col items-start gap-1 rounded-xl border border-border bg-card p-2.5 text-left"
            onclick={() => addNodeKind(k.slug)}
          >
            <span class="flex items-center gap-2 text-sm font-medium">
              <span class="size-1.5 rounded-full bg-primary transition-transform group-hover:scale-125"></span>
              {k.label}
            </span>
            <span class="text-[10px] tabular-nums text-muted-foreground">
              {k.inputs.length} in · {k.outputs.length} out
            </span>
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}
