<script lang="ts">
  // Unreal-style Variables panel (left rail of the editor). Declare typed, per-graph
  // variables; each row spawns Get/Set nodes for that variable onto the canvas.
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { typeColor } from "$lib/workflows/portColors";

  type Variable = { name: string; type: string; default?: string | null };

  let {
    variables = [],
    types,
    cameras = [],
    disabled = false,
    open = $bindable(true),
    onAdd,
    onEdit,
    onRemove,
  }: {
    variables: Variable[];
    types: { value: string; label: string }[];
    cameras?: { value: string; label: string }[];
    disabled?: boolean;
    open: boolean;
    onAdd: () => void;
    onEdit: (i: number, patch: Partial<Variable>) => void;
    onRemove: (i: number) => void;
  } = $props();
  // Which types accept an author-typed default (matches the engine seeding).
  const DEFAULTABLE = new Set(["text", "num", "bool", "camera", "recipient"]);

  // Resizable width (persisted to localStorage). Drag the right-edge handle.
  let width = $state(224);
  $effect(() => {
    const s = Number(localStorage.getItem("wf-vars-w"));
    if (s >= 180 && s <= 460) width = s;
  });
  function startResize(e: PointerEvent) {
    const sx = e.clientX;
    const sw = width;
    const move = (ev: PointerEvent) => {
      width = Math.max(180, Math.min(460, sw + (ev.clientX - sx)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      try { localStorage.setItem("wf-vars-w", String(width)); } catch { /* ignore */ }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    e.preventDefault();
  }
</script>

{#if !open}
  <button
    class="interactive group flex w-7 shrink-0 items-center justify-center rounded-xl border border-border bg-card/40 hover:bg-muted/40"
    title="Show variables"
    aria-label="Show variables"
    onclick={() => (open = true)}
  >
    <PanelLeftOpen class="size-4 text-muted-foreground group-hover:text-foreground" />
  </button>
{:else}
  <div
    class="relative flex shrink-0 flex-col gap-1.5 overflow-y-auto rounded-xl border border-border bg-card/40 p-2.5"
    style:width="{width}px"
  >
    <div class="flex items-center justify-between gap-2">
      <span class="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">Variables</span>
      <div class="flex items-center gap-0.5">
        <Button variant="ghost" size="icon" class="size-6" title="Add variable" disabled={disabled} onclick={onAdd}>
          <Plus class="size-3.5" />
        </Button>
        <Button variant="ghost" size="icon" class="size-6" title="Hide variables" onclick={() => (open = false)}>
          <PanelLeftClose class="size-3.5" />
        </Button>
      </div>
    </div>

    {#if variables.length === 0}
      <p class="rounded-lg border border-dashed border-border px-2 py-3 text-center text-[11px] leading-snug text-muted-foreground">
        No variables yet. Add one, then drag it onto the canvas (or use the add-node menu) to drop Get/Set.
      </p>
    {/if}

    <div class="space-y-1">
      {#each variables as v, i (i)}
        <!-- Compact one-line card: grip · name · type · delete -->
        <div class="flex items-center gap-1 rounded-md border border-border bg-background/60 px-1 py-0.5">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            class="shrink-0 cursor-grab"
            class:cursor-not-allowed={disabled}
            draggable={!disabled}
            title="Drag onto the canvas → Get / Set"
            style:color={typeColor(v.type)}
            ondragstart={(e) => e.dataTransfer?.setData("application/x-sentinel-variable", JSON.stringify({ name: v.name, type: v.type }))}
          >
            <GripVertical class="size-3.5" />
          </span>
          <Input
            class="h-6 min-w-0 flex-1 border-0 bg-transparent px-1 text-[11px] shadow-none focus-visible:ring-1"
            value={v.name}
            disabled={disabled}
            onchange={(e) => onEdit(i, { name: e.currentTarget.value.trim() })}
          />
          <select
            class="h-6 w-[68px] shrink-0 rounded border border-input bg-background px-1 text-[11px] disabled:opacity-50"
            value={v.type}
            disabled={disabled}
            onchange={(e) => onEdit(i, { type: e.currentTarget.value })}
          >
            {#each types as t (t.value)}<option value={t.value}>{t.label}</option>{/each}
          </select>
          <!-- default-value editor, per type (blank = no default → the Get's type-zero) -->
          {#if DEFAULTABLE.has(v.type)}
            {#if v.type === "bool"}
              <select
                class="h-6 w-[58px] shrink-0 rounded border border-input bg-background px-1 text-[11px] disabled:opacity-50"
                title="Default value"
                value={v.default ?? ""}
                disabled={disabled}
                onchange={(e) => onEdit(i, { default: e.currentTarget.value || null })}
              >
                <option value="">—</option><option value="true">true</option><option value="false">false</option>
              </select>
            {:else if v.type === "camera"}
              <select
                class="h-6 w-[84px] shrink-0 rounded border border-input bg-background px-1 text-[11px] disabled:opacity-50"
                title="Default camera"
                value={v.default ?? ""}
                disabled={disabled}
                onchange={(e) => onEdit(i, { default: e.currentTarget.value || null })}
              >
                <option value="">—</option>
                {#each cameras as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
              </select>
            {:else}
              <Input
                class="h-6 w-[70px] shrink-0 border-input bg-background px-1 text-[11px]"
                type={v.type === "num" ? "number" : "text"}
                placeholder="default"
                value={v.default ?? ""}
                disabled={disabled}
                onchange={(e) => onEdit(i, { default: e.currentTarget.value.trim() || null })}
              />
            {/if}
          {/if}
          <Button variant="ghost" size="icon" class="size-5 shrink-0 text-muted-foreground hover:text-destructive" title="Delete variable" disabled={disabled} onclick={() => onRemove(i)}>
            <Trash2 class="size-3" />
          </Button>
        </div>
      {/each}
    </div>

    <!-- Right-edge resize handle -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize hover:bg-primary/30"
      title="Drag to resize"
      onpointerdown={startResize}
    ></div>
  </div>
{/if}
