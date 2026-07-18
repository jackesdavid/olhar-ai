<script lang="ts">
  // A BIG modal to create or edit a dictionary live (#113) — used from a workflow variable row so
  // the user never leaves the editor to make one. Hosts a name field + the shared table-editor;
  // on save it persists and emits the saved dictionary so the caller can reference it.
  import { toast } from "svelte-sonner";
  import Table2 from "@lucide/svelte/icons/table-2";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import TableEditor from "$lib/components/table-editor.svelte";
  import { data, type Dictionary, type DictColumn, type DictCell } from "$lib/data";

  let {
    open = $bindable(false),
    dictId = null,
    onsaved,
  }: {
    open?: boolean;
    /** null → create a new dictionary; else load + edit this one. */
    dictId?: string | null;
    onsaved: (dict: Dictionary) => void;
  } = $props();

  let name = $state("");
  let columns = $state<DictColumn[]>([]);
  let rows = $state<DictCell[][]>([]);
  let loading = $state(false);
  let saving = $state(false);
  let loadedFor = $state<string | null | undefined>(undefined);

  // Re-seed whenever the modal opens (for a new target or a different dictId).
  $effect(() => {
    if (open && loadedFor !== dictId) {
      loadedFor = dictId;
      seed();
    }
    if (!open) loadedFor = undefined;
  });

  async function seed() {
    if (!dictId) {
      name = "";
      columns = [];
      rows = [];
      return;
    }
    loading = true;
    try {
      const d = await data.getDictionary(dictId);
      name = d.name;
      columns = d.columns.map((c) => ({ ...c }));
      rows = d.rows.map((r) => r.map((c) => ({ ...c }) as DictCell));
    } catch (err) {
      toast.error("Couldn't load dictionary", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  function onTableChange(c: DictColumn[], r: DictCell[][]) {
    columns = c;
    rows = r;
  }

  function saveDisabled(): boolean {
    return saving || loading || name.trim().length === 0;
  }

  async function save() {
    if (saveDisabled()) return;
    saving = true;
    const body = { name: name.trim(), columns, rows };
    try {
      let saved: Dictionary;
      if (dictId) {
        saved = await data.updateDictionary(dictId, body);
      } else {
        const id = await data.createDictionary(body);
        saved = await data.getDictionary(id);
      }
      toast.success(`Saved “${saved.name}”`);
      open = false;
      onsaved(saved);
    } catch (err) {
      toast.error("Couldn't save", { description: String(err) });
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="max-h-[85vh] gap-4 overflow-y-auto sm:max-w-3xl">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <Table2 class="size-4" /> {dictId ? "Edit dictionary" : "New dictionary"}
      </Dialog.Title>
      <Dialog.Description>Define the columns, then add rows of typed values.</Dialog.Description>
    </Dialog.Header>

    <div class="space-y-4">
      <div class="max-w-sm space-y-1.5">
        <label for="dict-modal-name" class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Name</label>
        <Input id="dict-modal-name" placeholder="e.g. Store hours" bind:value={name} />
      </div>
      {#if loading}
        <p class="text-sm text-muted-foreground">Loading…</p>
      {:else}
        <TableEditor {columns} {rows} onchange={onTableChange} />
      {/if}
    </div>

    <Dialog.Footer>
      <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
      <Button class="press" disabled={saveDisabled()} onclick={save}>
        {saving ? "Saving…" : "Save dictionary"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
