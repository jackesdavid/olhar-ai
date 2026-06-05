<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { toast } from "svelte-sonner";

  import Plus from "@lucide/svelte/icons/plus";
  import Play from "@lucide/svelte/icons/play";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Power from "@lucide/svelte/icons/power";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";

  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Badge } from "$lib/components/ui/badge";
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
  let paletteKind = $state<string>("");
  let pending = $state<{ node: number; port: string } | null>(null);
  let loading = $state(true);
  let running = $state(false);

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
        if (cat.kinds[0]) paletteKind = cat.kinds[0].slug;
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
      <Button size="sm" variant="outline" onclick={openCreate}>
        <Plus class="size-4" /> New
      </Button>
    </div>

    {#if loading}
      <Loader label="Loading…" size={32} />
    {:else if graphs.length === 0}
      <EmptyState title="No workflows yet" description="Create one to start automating.">
        <Button size="sm" onclick={openCreate}><Plus class="size-4" /> New workflow</Button>
      </EmptyState>
    {:else}
      <div class="space-y-1">
        {#each graphs as g (g.id)}
          <button
            class="flex w-full items-center justify-between rounded-lg border px-2.5 py-2 text-left text-sm transition-all duration-200 {selectedGraphId ===
            g.id
              ? 'border-primary/50 bg-primary/10 text-foreground'
              : 'border-border text-muted-foreground hover:translate-x-0.5 hover:bg-muted/50 hover:text-foreground'}"
            onclick={() => {
              selectedGraphId = g.id;
              selectedNodeId = null;
              pending = null;
            }}
          >
            <span class="flex items-center gap-2 truncate">
              <StatusDot state={g.enabled ? "online" : "offline"} />
              <span class="truncate">{g.name}</span>
            </span>
            <span class="rounded bg-muted px-1.5 text-[10px] tabular-nums text-muted-foreground">
              {g.nodes.length}
            </span>
          </button>
        {/each}
      </div>
    {/if}

    {#if graph}
      <div class="space-y-3 border-t border-border pt-3">
        <div class="flex items-center justify-between">
          <span class="text-xs font-medium uppercase tracking-wide text-muted-foreground">Settings</span>
          <div class="flex gap-1">
            <Button size="sm" variant="ghost" title="Rename" onclick={() => openRename(graph)}>
              <Pencil class="size-3.5" />
            </Button>
            <Button size="sm" variant="ghost" title="Delete" onclick={() => openDelete(graph)}>
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

        <Button class="w-full" onclick={() => runNow(graph)} disabled={running}>
          {#if running}
            <LoaderCircle class="size-4 animate-spin" /> Running…
          {:else}
            <Play class="size-4" /> Run now
          {/if}
        </Button>
      </div>
    {/if}
  </aside>

  <!-- Canvas + palette + config -->
  <section class="flex min-w-0 flex-1 flex-col gap-3">
    {#if !graph}
      <div class="flex flex-1 items-center justify-center">
        <EmptyState
          title="No workflow selected"
          description="Pick a workflow on the left, or create one to start wiring nodes."
        />
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

      <div
        class="relative min-h-0 flex-1 overflow-auto rounded-xl border border-border bg-card/20 bg-[radial-gradient(circle,_rgba(52,211,153,0.1)_1px,_transparent_1px)] [background-size:18px_18px]"
      >
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
      </div>

      <!-- selected node config -->
      {#if selectedNode}
        {@const k = catalog[selectedNode.kind]}
        <div class="rounded-xl border border-border bg-card/40 p-3">
          <div class="mb-2.5 flex items-center justify-between">
            <span class="flex items-center gap-2 text-sm font-medium">
              <span class="size-1.5 rounded-full bg-primary"></span>
              {k?.label ?? selectedNode.kind}
              <span class="text-muted-foreground">#{selectedNode.id}</span>
            </span>
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
      {/if}
    {/if}
  </section>
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
