<script lang="ts">
  // A typed-column TABLE editor (#113) — the structured cousin of dictionary-editor. Define
  // columns (name + type: Text/Num/Bool) = the structure, then add rows with a typed cell per
  // column. Fully controlled: it manages local $state and emits the rebuilt (columns, rows) on
  // every change; the parent owns the name and persistence.
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import { Switch } from "$lib/components/ui/switch";
  import * as Select from "$lib/components/ui/select";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import type { CellType, DictColumn, DictCell } from "$lib/data";

  let {
    columns = [],
    rows = [],
    disabled = false,
    onchange,
  }: {
    columns?: DictColumn[];
    rows?: DictCell[][];
    disabled?: boolean;
    onchange: (columns: DictColumn[], rows: DictCell[][]) => void;
  } = $props();

  // Local working copies — deep-cloned so edits don't mutate the parent's arrays until we emit.
  let cols = $state<DictColumn[]>(columns.map((c) => ({ ...c })));
  let data = $state<DictCell[][]>(rows.map((r) => r.map((c) => ({ ...c }) as DictCell)));

  const TYPE_OPTIONS: { value: CellType; label: string }[] = [
    { value: "text", label: "Text" },
    { value: "num", label: "Number" },
    { value: "bool", label: "Bool" },
  ];

  function zeroCell(type: CellType): DictCell {
    if (type === "num") return { t: "num", v: 0 };
    if (type === "bool") return { t: "bool", v: false };
    return { t: "text", v: "" };
  }

  function coerceCell(cell: DictCell, type: CellType): DictCell {
    if (type === "num") {
      const n = typeof cell.v === "number" ? cell.v : parseInt(String(cell.v), 10);
      return { t: "num", v: Number.isFinite(n) ? n : 0 };
    }
    if (type === "bool") {
      const b = cell.t === "bool" ? cell.v : ["true", "1", "yes", "on"].includes(String(cell.v).trim());
      return { t: "bool", v: b };
    }
    return { t: "text", v: cell.t === "bool" ? String(cell.v) : String(cell.v ?? "") };
  }

  function typeLabel(type: CellType): string {
    return TYPE_OPTIONS.find((o) => o.value === type)?.label ?? "Text";
  }

  function emit() {
    onchange(
      cols.map((c) => ({ ...c })),
      data.map((r) => r.map((c) => ({ ...c }) as DictCell)),
    );
  }

  function addColumn() {
    cols = [...cols, { name: `col${cols.length + 1}`, type: "text" }];
    data = data.map((r) => [...r, zeroCell("text")]);
    emit();
  }
  function removeColumn(ci: number) {
    cols = cols.filter((_, j) => j !== ci);
    data = data.map((r) => r.filter((_, j) => j !== ci));
    emit();
  }
  function renameColumn(ci: number, name: string) {
    cols[ci] = { ...cols[ci], name };
    emit();
  }
  function retypeColumn(ci: number, type: CellType) {
    cols[ci] = { ...cols[ci], type };
    data = data.map((r) => r.map((cell, j) => (j === ci ? coerceCell(cell, type) : cell)));
    emit();
  }

  function addRow() {
    data = [...data, cols.map((c) => zeroCell(c.type))];
    emit();
  }
  function removeRow(ri: number) {
    data = data.filter((_, j) => j !== ri);
    emit();
  }
  function setCell(ri: number, ci: number, cell: DictCell) {
    data[ri][ci] = cell;
    emit();
  }
</script>

<div class="space-y-3">
  {#if cols.length === 0}
    <div class="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border py-8 text-center">
      <Columns3 class="size-5 text-muted-foreground" />
      <p class="text-sm text-muted-foreground">Start by adding a column to define the structure.</p>
      <Button variant="outline" size="sm" class="gap-1.5" {disabled} onclick={addColumn}>
        <Plus class="size-3.5" /> Add column
      </Button>
    </div>
  {:else}
    <div class="overflow-x-auto rounded-lg border border-border">
      <table class="w-full border-collapse text-sm">
        <thead>
          <tr class="border-b border-border bg-muted/40">
            {#each cols as col, ci (ci)}
              <th class="min-w-[9rem] border-r border-border p-1.5 align-top">
                <div class="flex flex-col gap-1">
                  <Input
                    class="h-7 text-xs font-medium"
                    placeholder="column"
                    value={col.name}
                    {disabled}
                    oninput={(e) => renameColumn(ci, e.currentTarget.value)}
                  />
                  <div class="flex items-center gap-1">
                    <Select.Root
                      type="single"
                      value={col.type}
                      {disabled}
                      onValueChange={(v) => retypeColumn(ci, (v ?? "text") as CellType)}
                    >
                      <Select.Trigger class="h-6 flex-1 text-[11px]">{typeLabel(col.type)}</Select.Trigger>
                      <Select.Content>
                        {#each TYPE_OPTIONS as o (o.value)}
                          <Select.Item value={o.value} label={o.label}>{o.label}</Select.Item>
                        {/each}
                      </Select.Content>
                    </Select.Root>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6 shrink-0 text-muted-foreground hover:text-destructive"
                      title="Remove column"
                      {disabled}
                      onclick={() => removeColumn(ci)}
                    >
                      <X class="size-3" />
                    </Button>
                  </div>
                </div>
              </th>
            {/each}
            <th class="w-9 p-1.5 align-top">
              <Button
                variant="ghost"
                size="icon"
                class="size-7 text-muted-foreground"
                title="Add column"
                {disabled}
                onclick={addColumn}
              >
                <Plus class="size-3.5" />
              </Button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#each data as row, ri (ri)}
            <tr class="border-b border-border last:border-b-0">
              {#each cols as col, ci (ci)}
                <td class="border-r border-border p-1">
                  {#if col.type === "bool"}
                    <div class="flex h-8 items-center justify-center">
                      <Switch
                        checked={row[ci]?.t === "bool" ? row[ci].v : false}
                        {disabled}
                        onCheckedChange={(v) => setCell(ri, ci, { t: "bool", v })}
                      />
                    </div>
                  {:else if col.type === "num"}
                    <Input
                      type="number"
                      class="h-8 border-0 shadow-none focus-visible:ring-1"
                      value={row[ci]?.v ?? 0}
                      {disabled}
                      oninput={(e) => setCell(ri, ci, { t: "num", v: parseInt(e.currentTarget.value, 10) || 0 })}
                    />
                  {:else}
                    <Input
                      class="h-8 border-0 shadow-none focus-visible:ring-1"
                      value={String(row[ci]?.v ?? "")}
                      {disabled}
                      oninput={(e) => setCell(ri, ci, { t: "text", v: e.currentTarget.value })}
                    />
                  {/if}
                </td>
              {/each}
              <td class="p-1 text-center">
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-7 text-muted-foreground hover:text-destructive"
                  title="Remove row"
                  {disabled}
                  onclick={() => removeRow(ri)}
                >
                  <X class="size-3.5" />
                </Button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <Button variant="outline" size="sm" class="gap-1.5" {disabled} onclick={addRow}>
      <Plus class="size-3.5" /> Add row
    </Button>
  {/if}
</div>
