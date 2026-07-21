<script lang="ts">
  // Dictionaries — a full view (not a modal): browse the tenant's typed-column tables and
  // create/edit/remove them. Editing opens a full-page editor (name + table-editor), in keeping
  // with the "complete views, not modals" house rule. Card #113.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import Table2 from "@lucide/svelte/icons/table-2";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Rows3 from "@lucide/svelte/icons/rows-3";
  import PageHeader from "$lib/components/page-header.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Loader from "$lib/components/loader.svelte";
  import IconChip from "$lib/components/icon-chip.svelte";
  import TableEditor from "$lib/components/table-editor.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { data, type Dictionary, type DictColumn, type DictCell } from "$lib/data";

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
      if (editing.id) {
        await data.updateDictionary(editing.id, body);
      } else {
        await data.createDictionary(body);
      }
      toast.success(`Saved “${body.name}”`);
      editing = null;
      await load();
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
    } catch (err) {
      toast.error("Couldn't delete", { description: String(err) });
    }
  }
</script>

<div class="space-y-6">
  {#if editing}
    <PageHeader
      title={editing.id ? "Edit dictionary table" : "New dictionary table"}
      description="Define the columns (the structure), then add rows of typed values."
      icon={Table2}
    >
      {#snippet actions()}
        <Button variant="ghost" onclick={cancelEdit}>Cancel</Button>
        <Button class="press" disabled={saveDisabled()} onclick={save}>
          {saving ? "Saving…" : "Save dictionary table"}
        </Button>
      {/snippet}
    </PageHeader>

    <div class="space-y-4">
      <div class="max-w-sm space-y-1.5">
        <label for="dict-name" class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Name</label>
        <Input id="dict-name" placeholder="e.g. Store hours" bind:value={editing.name} />
      </div>
      <TableEditor columns={editing.columns} rows={editing.rows} {cameras} {recipients} onchange={onTableChange} />
    </div>
  {:else}
    <PageHeader
      title="Dictionary Tables"
      description="Reusable typed tables — reference one from a workflow variable; edit it once and every workflow sees it."
      icon={Table2}
    >
      {#snippet actions()}
        <Button variant="outline" onclick={() => goto("/workflows")}>
          <ArrowLeft class="size-4" /> Workflows
        </Button>
        <Button class="press" onclick={startNew}>
          <Plus class="size-4" /> New dictionary table
        </Button>
      {/snippet}
    </PageHeader>

    {#if loading}
      <Loader label="Loading dictionary tables…" />
    {:else if dicts.length === 0}
      <EmptyState
        title="No dictionary tables yet"
        description="Create a typed table of values you can reference from workflow variables and mutate through graphs."
      >
        {#snippet children()}
          <Button class="press" onclick={startNew}>
            <Plus class="size-4" /> New dictionary table
          </Button>
        {/snippet}
      </EmptyState>
    {:else}
      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {#each dicts as d (d.id)}
          <div class="interactive group flex flex-col rounded-2xl border border-border bg-card p-5">
            <div class="mb-3 flex items-start justify-between gap-3">
              <IconChip>
                {#snippet children()}<Table2 class="size-5" />{/snippet}
              </IconChip>
              <Button
                variant="ghost"
                size="icon"
                class="size-8 text-muted-foreground opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100 [@media(hover:none)]:opacity-100"
                title="Delete dictionary table"
                onclick={(e) => { e.stopPropagation(); remove(d); }}
              >
                <Trash2 class="size-4" />
              </Button>
            </div>
            <button class="text-left" onclick={() => startEdit(d)}>
              <h3 class="text-[15px] font-semibold tracking-tight">{d.name}</h3>
              <div class="mt-2 flex items-center gap-3 text-xs text-muted-foreground">
                <span class="inline-flex items-center gap-1"><Columns3 class="size-3.5" /> {d.columns.length} cols</span>
                <span class="inline-flex items-center gap-1"><Rows3 class="size-3.5" /> {d.rows.length} rows</span>
              </div>
            </button>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
