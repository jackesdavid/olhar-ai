<script lang="ts">
  // An editor for an arbitrary string→string dictionary (e.g. HTTP headers / form fields).
  // Manages its own row state and emits the rebuilt object on change. Remount it
  // (via {#key}) when the underlying node/port changes so it re-seeds cleanly.
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";

  let {
    value = {},
    disabled = false,
    onchange,
  }: {
    value?: Record<string, unknown>;
    disabled?: boolean;
    onchange: (v: Record<string, string>) => void;
  } = $props();

  type Row = { k: string; v: string };
  let rows = $state<Row[]>(
    Object.entries(value ?? {}).map(([k, v]) => ({ k, v: String(v ?? "") })),
  );

  function emit() {
    const obj: Record<string, string> = {};
    for (const r of rows) {
      const key = r.k.trim();
      if (key) obj[key] = r.v;
    }
    onchange(obj);
  }
  function add() {
    rows = [...rows, { k: "", v: "" }];
  }
  function remove(i: number) {
    rows = rows.filter((_, j) => j !== i);
    emit();
  }
</script>

<div class="space-y-1.5">
  {#each rows as row, i (i)}
    <div class="flex items-center gap-1">
      <Input class="h-8 min-w-0 flex-1" placeholder="key" {disabled} bind:value={row.k} onblur={emit} />
      <Input class="h-8 min-w-0 flex-1" placeholder="value" {disabled} bind:value={row.v} onblur={emit} />
      <Button variant="ghost" size="icon" class="size-7 shrink-0 text-muted-foreground hover:text-destructive" {disabled} onclick={() => remove(i)}>
        <X class="size-3.5" />
      </Button>
    </div>
  {/each}
  <Button variant="outline" size="sm" class="h-7 w-full gap-1.5 text-xs" {disabled} onclick={add}>
    <Plus class="size-3" /> Add entry
  </Button>
</div>
