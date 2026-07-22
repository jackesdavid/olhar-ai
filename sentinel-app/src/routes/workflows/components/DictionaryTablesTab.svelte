<script lang="ts">
  // The Dictionary Tables tab of the workflows list — browse the tenant's typed-column tables and
  // create/edit/remove them, as a row list matching the other tabs. Self-contained (loads its own
  // data); `onChanged` lets the parent refresh the node-config dictionary dropdowns. Extracted from
  // the /workflows/dictionaries route so the giant editor page doesn't grow (#113/#137).
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";
  import Table2 from "@lucide/svelte/icons/table-2";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Rows3 from "@lucide/svelte/icons/rows-3";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Loader from "$lib/components/loader.svelte";
  import TableEditor from "$lib/components/table-editor.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { data, type Dictionary, type DictColumn, type DictCell } from "$lib/data";

  let { onChanged }: { onChanged?: () => void } = $props();

  let dicts = $state<Dictionary[]>([]);
  let cameras = $state<{ value: string; label: string }[]>([]);
  let recipients = $state<{ value: string; label: string }[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  // Editor mode: null = list; else the dictionary being edited (id null = a brand-new one).
  let editing = $state<{ id: string | null; name: string; columns: DictColumn[]; rows: DictCell[][] } | null>(null);

  onMount(load);
  async function load() {
    loading = true;
    try {
      const [ds, devs, beacons] = await Promise.all([
        data.listDictionaries(),
        data.listDevices().catch(() => []),
        data.listBeacons().catch(() => []),
      ]);
      dicts = ds;
      cameras = devs.map((d) => ({ value: d.id || d.ip || d.name, label: d.name }));
      recipients = beacons.map((b) => ({ value: b.label, label: b.label }));
    } catch (err) {
      toast.error("Couldn't load dictionary tables", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  function startNew() {
    editing = { id: null, name: "", columns: [], rows: [] };
  }
  function startEdit(d: Dictionary) {
    editing = {
      id: d.id,
      name: d.name,
      columns: d.columns.map((c) => ({ ...c })),
      rows: d.rows.map((r) => r.map((c) => ({ ...c }) as DictCell)),
    };
  }
  function cancelEdit() {
    editing = null;
  }
  function onTableChange(columns: DictColumn[], rows: DictCell[][]) {
    if (editing) editing = { ...editing, columns, rows };
  }
  function saveDisabled(): boolean {
    return saving || !editing || editing.name.trim().length === 0;
  }
  async function save() {
    if (!editing || saveDisabled()) return;
    saving = true;
    const body = { name: editing.name.trim(), columns: editing.columns, rows: editing.rows };
    try {
      if (editing.id) await data.updateDictionary(editing.id, body);
      else await data.createDictionary(body);
      toast.success(`Saved “${body.name}”`);
      editing = null;
      await load();
      onChanged?.();
    } catch (err) {
      toast.error("Couldn't save", { description: String(err) });
    } finally {
      saving = false;
    }
  }
  async function remove(d: Dictionary) {
    try {
      await data.deleteDictionary(d.id);
      dicts = dicts.filter((x) => x.id !== d.id);
      toast.success(`Deleted “${d.name}”`);
      onChanged?.();
    } catch (err) {
      toast.error("Couldn't delete", { description: String(err) });
    }
  }
</script>

{#if editing}
  <div class="min-h-0 flex-1 space-y-4 overflow-y-auto">
    <div class="flex items-center justify-between gap-3">
      <Button variant="outline" size="sm" onclick={cancelEdit}>
        <ArrowLeft class="size-4" /> Tables
      </Button>
      <Button class="press" size="sm" disabled={saveDisabled()} onclick={save}>
        {saving ? "Saving…" : editing.id ? "Save table" : "Create table"}
      </Button>
    </div>
    <div class="max-w-sm space-y-1.5">
      <label for="dict-tab-name" class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Name</label>
      <Input id="dict-tab-name" placeholder="e.g. Store hours" bind:value={editing.name} />
    </div>
    <TableEditor columns={editing.columns} rows={editing.rows} {cameras} {recipients} onchange={onTableChange} />
  </div>
{:else}
  <div class="min-h-0 flex-1 space-y-2 overflow-y-auto">
    <div class="flex justify-end">
      <Button class="press" size="sm" onclick={startNew}>
        <Plus class="size-4" /> New table
      </Button>
    </div>
    {#if loading}
      <div class="rounded-xl border border-border p-12 text-center"><Loader label="Loading dictionary tables…" /></div>
    {:else if dicts.length === 0}
      <div class="rounded-xl border border-border p-6">
        <EmptyState
          title="No dictionary tables yet"
          description="Create a typed table of values you can reference from workflow variables and mutate through graphs."
        >
          <Button class="press" onclick={startNew}><Plus class="size-4" /> New dictionary table</Button>
        </EmptyState>
      </div>
    {:else}
      <div class="overflow-hidden rounded-xl border border-border">
        <div class="divide-y divide-border/60">
          {#each dicts as d (d.id)}
            <div class="group flex items-center gap-3 px-3 py-2.5 transition-colors hover:bg-primary/5">
              <div class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                <Table2 class="size-4" />
              </div>
              <button class="min-w-0 flex-1 text-left" onclick={() => startEdit(d)}>
                <span class="block truncate text-sm font-medium">{d.name}</span>
                <span class="mt-0.5 flex items-center gap-3 text-xs text-muted-foreground">
                  <span class="inline-flex items-center gap-1"><Columns3 class="size-3.5" /> {d.columns.length} cols</span>
                  <span class="inline-flex items-center gap-1"><Rows3 class="size-3.5" /> {d.rows.length} rows</span>
                </span>
              </button>
              <Button
                variant="ghost"
                size="icon"
                class="size-8 shrink-0 text-muted-foreground opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100 [@media(hover:none)]:opacity-100"
                title="Delete dictionary table"
                onclick={(e) => { e.stopPropagation(); remove(d); }}
              >
                <Trash2 class="size-4" />
              </Button>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
{/if}
