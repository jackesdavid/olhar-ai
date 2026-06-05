<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { toast } from "svelte-sonner";

  import Plus from "@lucide/svelte/icons/plus";
  import Play from "@lucide/svelte/icons/play";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Workflow from "@lucide/svelte/icons/workflow";
  import Power from "@lucide/svelte/icons/power";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Badge } from "$lib/components/ui/badge";

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
  let paletteKind = $state<string>("");
  let pending = $state<{ node: number; port: string } | null>(null);
  let loading = $state(true);

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
        if (cat.kinds[0]) paletteKind = cat.kinds[0].slug;
        await reload();
      } catch (err) {
        toast.error("Failed to load workflows", { description: String(err) });
      } finally {
        loading = false;
      }
    })();
    const un = listen<{ name: string; ok: boolean; summary: string }>("workflow-run", (e) => {
      const { name, ok, summary } = e.payload;
      if (ok) toast.success(`Workflow "${name}"`, { description: summary });
      else toast.error(`Workflow "${name}" failed`, { description: summary });
    });
    return () => un.then((u) => u());
  });

  // ---- graph CRUD ----------------------------------------------------------
  async function createGraph() {
    const name = prompt("New workflow name?", "Untitled")?.trim();
    if (name === undefined) return;
    const id = await invoke<number>("create_graph", { name: name || "Untitled" });
    selectedGraphId = id;
    selectedNodeId = null;
    await reload();
  }
  async function renameGraph(g: Graph) {
    const name = prompt("Rename workflow", g.name)?.trim();
    if (!name) return;
    await invoke("rename_graph", { id: g.id, name });
    await reload();
  }
  async function deleteGraph(g: Graph) {
    if (!confirm(`Delete workflow "${g.name}"?`)) return;
    await invoke("delete_graph", { id: g.id });
    if (selectedGraphId === g.id) selectedGraphId = null;
    await reload();
  }
  async function toggleEnabled(g: Graph, enabled: boolean) {
    await invoke("set_graph_enabled", { id: g.id, enabled });
    await reload();
  }
  async function runNow(g: Graph) {
    try {
      const msg = await invoke<string>("run_graph", { id: g.id });
      toast.info(msg);
    } catch (err) {
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
  async function addNode() {
    if (!graph || !paletteKind) return;
    const x = 40 + (graph.nodes.length % 4) * 40;
    const y = 40 + (graph.nodes.length % 4) * 30;
    const id = await invoke<number>("add_node", { graphId: graph.id, kind: paletteKind, x, y });
    selectedNodeId = id;
    await reload();
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

<div class="flex h-[calc(100vh-3.5rem)] gap-4">
  <!-- Sidebar: graph list + settings -->
  <aside class="flex w-72 shrink-0 flex-col gap-4 overflow-y-auto border-r border-border pr-4">
    <div class="flex items-center justify-between">
      <h1 class="text-lg font-semibold tracking-tight">Workflows</h1>
      <Button size="sm" variant="outline" onclick={createGraph}>
        <Plus class="size-4" /> New
      </Button>
    </div>

    {#if loading}
      <p class="text-sm text-muted-foreground">Loading…</p>
    {:else if graphs.length === 0}
      <p class="flex flex-col items-center gap-2 py-8 text-center text-sm text-muted-foreground">
        <Workflow class="size-6 opacity-50" />
        No workflows yet. Create one to start automating.
      </p>
    {:else}
      <div class="space-y-1">
        {#each graphs as g (g.id)}
          <button
            class="flex w-full items-center justify-between rounded-md border px-2.5 py-2 text-left text-sm transition-colors {selectedGraphId === g.id ? 'border-primary bg-muted' : 'border-border hover:bg-muted/50'}"
            onclick={() => { selectedGraphId = g.id; selectedNodeId = null; pending = null; }}
          >
            <span class="flex items-center gap-2 truncate">
              <span class="size-2 shrink-0 rounded-full {g.enabled ? 'bg-emerald-400' : 'bg-muted-foreground/40'}"></span>
              <span class="truncate">{g.name}</span>
            </span>
            <span class="text-xs text-muted-foreground">{g.nodes.length}n</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if graph}
      <div class="space-y-3 border-t border-border pt-3">
        <div class="flex items-center justify-between">
          <span class="text-xs font-medium uppercase tracking-wide text-muted-foreground">Settings</span>
          <div class="flex gap-1">
            <Button size="sm" variant="ghost" title="Rename" onclick={() => renameGraph(graph)}>
              <Pencil class="size-3.5" />
            </Button>
            <Button size="sm" variant="ghost" title="Delete" onclick={() => deleteGraph(graph)}>
              <Trash2 class="size-3.5" />
            </Button>
          </div>
        </div>

        <div class="flex items-center justify-between">
          <Label class="flex items-center gap-1.5 text-sm"><Power class="size-3.5" /> Enabled</Label>
          <Switch checked={graph.enabled} onCheckedChange={(v) => toggleEnabled(graph, v)} />
        </div>

        <!-- Trigger -->
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

        <!-- Concurrency -->
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

        <Button class="w-full" onclick={() => runNow(graph)}>
          <Play class="size-4" /> Run now
        </Button>
      </div>
    {/if}
  </aside>

  <!-- Canvas + palette + config -->
  <section class="flex min-w-0 flex-1 flex-col gap-3">
    {#if !graph}
      <div class="flex flex-1 items-center justify-center text-sm text-muted-foreground">
        Select or create a workflow.
      </div>
    {:else}
      <div class="flex flex-wrap items-center gap-2">
        <select bind:value={paletteKind} class="h-9 rounded-md border border-input bg-transparent px-2 text-sm">
          {#each kinds as k}<option value={k.slug}>{k.label}</option>{/each}
        </select>
        <Button size="sm" variant="outline" onclick={addNode}><Plus class="size-4" /> Add node</Button>
        {#if pending}
          <Badge variant="outline" class="border-primary/40 text-primary">
            wiring from #{pending.node}.{pending.port} — click an input port
          </Badge>
          <Button size="sm" variant="ghost" onclick={() => (pending = null)}>cancel</Button>
        {:else}
          <span class="text-xs text-muted-foreground">Click an output ●, then an input ○ to wire. Click a wired input to unwire.</span>
        {/if}
      </div>

      <div class="min-h-0 flex-1 overflow-auto rounded-lg border border-border bg-[radial-gradient(circle,_theme(colors.border)_1px,_transparent_1px)] [background-size:16px_16px]">
        <svg
          width="1600"
          height="900"
          class="block"
          onpointermove={onDragMove}
          onpointerup={endDrag}
          onpointerleave={endDrag}
          role="presentation"
        >
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
                d={`M ${x1} ${y1} C ${x1 + 50} ${y1}, ${x2 - 50} ${y2}, ${x2} ${y2}`}
                fill="none"
                stroke="#71717a"
                stroke-width="2"
                class="cursor-pointer hover:stroke-red-400"
                onclick={() => removeEdge(idx)}
              />
            {/if}
          {/each}

          <!-- nodes -->
          {#each graph.nodes as n (n.id)}
            {@const k = catalog[n.kind]}
            {@const h = nodeHeight(n)}
            <g>
              <rect
                x={n.x} y={n.y} width={NODE_W} height={h} rx="6"
                fill="hsl(240 6% 12%)"
                stroke={selectedNodeId === n.id ? "hsl(220 90% 60%)" : "hsl(240 5% 26%)"}
                stroke-width={selectedNodeId === n.id ? 2 : 1}
              />
              <rect
                x={n.x} y={n.y} width={NODE_W} height={HEAD_H} rx="6"
                fill="hsl(240 6% 16%)" class="cursor-move"
                onpointerdown={(ev) => startDrag(ev, n)}
                role="presentation"
              />
              <text x={n.x + 8} y={n.y + 17} class="pointer-events-none select-none fill-zinc-200" font-size="11" font-weight="600">
                {k?.label ?? n.kind}
              </text>
              <!-- input ports -->
              {#each k?.inputs ?? [] as p, i}
                {@const wired = wiredInto(n.id, p.name)}
                <circle
                  cx={n.x} cy={inPortY(n, i)} r="5"
                  fill={wired ? portColor[p.type] : "hsl(240 6% 12%)"}
                  stroke={portColor[p.type]} stroke-width="2"
                  class="cursor-pointer"
                  onclick={() => clickInPort(n, p.name)}
                  role="presentation"
                />
                <text x={n.x + 10} y={inPortY(n, i) + 3} class="pointer-events-none select-none fill-zinc-400" font-size="10">
                  {p.name}{p.required ? "*" : ""}
                </text>
              {/each}
              <!-- output ports -->
              {#each k?.outputs ?? [] as p, i}
                <circle
                  cx={n.x + NODE_W} cy={outPortY(n, i)} r="5"
                  fill={pending && pending.node === n.id && pending.port === p.name ? portColor[p.type] : "hsl(240 6% 12%)"}
                  stroke={portColor[p.type]} stroke-width="2"
                  class="cursor-pointer"
                  onclick={() => clickOutPort(n, p.name)}
                  role="presentation"
                />
                <text x={n.x + NODE_W - 10} y={outPortY(n, i) + 3} text-anchor="end" class="pointer-events-none select-none fill-zinc-400" font-size="10">
                  {p.name}
                </text>
              {/each}
            </g>
          {/each}
        </svg>
      </div>

      <!-- selected node config -->
      {#if selectedNode}
        {@const k = catalog[selectedNode.kind]}
        <div class="rounded-lg border border-border p-3">
          <div class="mb-2 flex items-center justify-between">
            <span class="text-sm font-medium">{k?.label ?? selectedNode.kind} <span class="text-muted-foreground">#{selectedNode.id}</span></span>
            <Button size="sm" variant="ghost" onclick={() => removeNode(selectedNode)}>
              <Trash2 class="size-3.5" /> Delete node
            </Button>
          </div>
          {#if Object.keys(selectedNode.config).length === 0}
            <p class="text-xs text-muted-foreground">This node has no settings.</p>
          {:else}
            <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
              {#each Object.keys(selectedNode.config) as key (key)}
                {@const wired = wiredInto(selectedNode.id, key)}
                <div class="space-y-1">
                  <Label class="text-xs text-muted-foreground">{key}</Label>
                  {#if wired}
                    <p class="rounded border border-dashed border-border px-2 py-1.5 text-xs text-muted-foreground">← #{wired.from}.{wired.from_port}</p>
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
      {/if}
    {/if}
  </section>
</div>
