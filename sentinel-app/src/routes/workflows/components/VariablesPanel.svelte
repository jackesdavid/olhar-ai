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

  type Variable = { name: string; type: string };

  let {
    variables = [],
    types,
    disabled = false,
    open = $bindable(true),
    onAdd,
    onEdit,
    onRemove,
  }: {
    variables: Variable[];
    types: { value: string; label: string }[];
    disabled?: boolean;
    open: boolean;
    onAdd: () => void;
    onEdit: (i: number, patch: Partial<Variable>) => void;
    onRemove: (i: number) => void;
  } = $props();
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
  <div class="flex w-56 shrink-0 flex-col gap-2 overflow-y-auto rounded-xl border border-border bg-card/40 p-3">
    <div class="flex items-center justify-between gap-2">
      <span class="text-xs font-medium uppercase tracking-wide text-muted-foreground">Variables</span>
      <div class="flex items-center gap-1">
        <Button variant="ghost" size="icon" class="size-7" title="Add variable" disabled={disabled} onclick={onAdd}>
          <Plus class="size-3.5" />
        </Button>
        <Button variant="ghost" size="icon" class="size-7" title="Hide variables" onclick={() => (open = false)}>
          <PanelLeftClose class="size-3.5" />
        </Button>
      </div>
    </div>

    {#if variables.length === 0}
      <p class="rounded-lg border border-dashed border-border px-2 py-3 text-center text-[11px] leading-snug text-muted-foreground">
        No variables yet. Add one, then drag it onto the canvas (or use the add-node menu) to drop Get/Set.
      </p>
    {/if}

    <div class="space-y-2">
      {#each variables as v, i (i)}
        <div class="space-y-1.5 rounded-lg border border-border bg-background/60 p-2">
          <div class="flex items-center gap-1.5">
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
              class="h-7 flex-1 text-xs"
              value={v.name}
              disabled={disabled}
              onchange={(e) => onEdit(i, { name: e.currentTarget.value.trim() })}
            />
            <Button variant="ghost" size="icon" class="size-6 text-muted-foreground hover:text-destructive" title="Delete variable" disabled={disabled} onclick={() => onRemove(i)}>
              <Trash2 class="size-3" />
            </Button>
          </div>
          <select
            class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs disabled:opacity-50"
            value={v.type}
            disabled={disabled}
            onchange={(e) => onEdit(i, { type: e.currentTarget.value })}
          >
            {#each types as t (t.value)}<option value={t.value}>{t.label}</option>{/each}
          </select>
        </div>
      {/each}
    </div>
  </div>
{/if}
