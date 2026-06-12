<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { toast } from "svelte-sonner";

  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Users from "@lucide/svelte/icons/users";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ScanFace from "@lucide/svelte/icons/scan-face";
  import Radio from "@lucide/svelte/icons/radio";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import Loader from "$lib/components/loader.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import type { Person, PersonEmbedding, NotificationTarget } from "$lib/types";

  const id = $derived($page.params.id);
  const isNew = $derived(id === "new");

  let people = $state<Person[]>([]);
  let recipients = $state<NotificationTarget[]>([]);
  let permissionKinds = $state<string[]>([]);
  let embeddings = $state<PersonEmbedding[]>([]);
  let loading = $state(true);
  let notFound = $state(false);
  let saving = $state(false);
  let deleting = $state(false);
  let deleteOpen = $state(false);

  let form = $state<Person>(blankPerson());

  const approvedFaces = $derived(embeddings.filter((e) => e.approved).length);

  const NONE = "__none__"; // Select sentinel for "no parent / no recipient"

  function blankPerson(): Person {
    return { id: "", name: "", parent_id: null, recipient_label: null, permissions: [] };
  }

  function permLabel(slug: string): string {
    const s = slug.replace(/_/g, " ");
    return s.charAt(0).toUpperCase() + s.slice(1);
  }
  function personName(pid: string | null): string {
    if (!pid) return "None";
    return people.find((p) => p.id === pid)?.name ?? "None";
  }
  // A person can't be its own guardian; the backend also rejects deeper cycles.
  function parentOptions(): Person[] {
    return people.filter((p) => p.id !== form.id);
  }
  function subordinates(): Person[] {
    return isNew ? [] : people.filter((p) => p.parent_id === form.id);
  }
  function hasPerm(slug: string): boolean {
    return form.permissions.includes(slug);
  }
  function togglePerm(slug: string, on: boolean) {
    form.permissions = on
      ? [...form.permissions, slug]
      : form.permissions.filter((p) => p !== slug);
  }

  async function load() {
    loading = true;
    notFound = false;
    try {
      [people, recipients, permissionKinds] = await Promise.all([
        invoke<Person[]>("list_people"),
        invoke<NotificationTarget[]>("list_beacons"),
        invoke<string[]>("person_permission_kinds"),
      ]);
      if (isNew) {
        form = blankPerson();
      } else {
        const found = people.find((p) => p.id === id);
        if (!found) {
          notFound = true;
        } else {
          form = { ...found, permissions: [...found.permissions] };
          embeddings = await invoke<PersonEmbedding[]>("list_embeddings", { personId: id });
        }
      }
    } catch (err) {
      console.error("Failed to load person", err);
      toast.error("Failed to load person", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function save() {
    if (form.name.trim().length === 0) {
      toast.error("Name can't be empty.");
      return;
    }
    saving = true;
    try {
      const person: Person = {
        ...form,
        id: form.id || crypto.randomUUID(),
        name: form.name.trim(),
      };
      await invoke("save_person", { person });
      toast.success(isNew ? "Person added." : "Person updated.");
      await goto("/people/directory");
    } catch (err) {
      console.error("Save failed", err);
      toast.error("Save failed", { description: String(err) });
    } finally {
      saving = false;
    }
  }

  async function doDelete() {
    deleting = true;
    try {
      await invoke("delete_person", { id: form.id });
      toast.success("Person removed.");
      await goto("/people/directory");
    } catch (err) {
      console.error("Delete failed", err);
      toast.error("Delete failed", { description: String(err) });
    } finally {
      deleting = false;
    }
  }
</script>

<div class="mx-auto max-w-3xl space-y-6">
  <!-- top bar -->
  <div class="flex items-center justify-between gap-4">
    <div class="flex min-w-0 items-center gap-3">
      <Button variant="ghost" size="icon" title="Back to People" onclick={() => goto("/people/directory")}>
        <ArrowLeft class="size-4" />
      </Button>
      <div class="min-w-0">
        <h1 class="truncate text-2xl font-semibold tracking-tight">
          {isNew ? "New person" : form.name || "Person"}
        </h1>
        <p class="text-sm text-muted-foreground">
          {isNew ? "Create a record — face enrollment comes later." : "Edit this person's record."}
        </p>
      </div>
    </div>
    {#if !loading && !notFound}
      <div class="flex shrink-0 items-center gap-2">
        {#if !isNew}
          <Button variant="outline" onclick={() => (deleteOpen = true)}>
            <Trash2 class="size-4" /> Delete
          </Button>
        {/if}
        <Button onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</Button>
      </div>
    {/if}
  </div>

  {#if loading}
    <Loader label="Loading…" />
  {:else if notFound}
    <Card.Root>
      <Card.Content class="py-10 text-center text-sm text-muted-foreground">
        That person no longer exists.
        <div class="mt-4">
          <Button variant="outline" onclick={() => goto("/people/directory")}>Back to People</Button>
        </div>
      </Card.Content>
    </Card.Root>
  {:else}
    <!-- Details -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><Users class="size-4 text-muted-foreground" /> Details</Card.Title>
      </Card.Header>
      <Card.Content>
        <div class="space-y-2">
          <Label for="person-name">Name</Label>
          <Input id="person-name" bind:value={form.name} placeholder="Full name" autocomplete="off" />
        </div>
      </Card.Content>
    </Card.Root>

    <!-- Hierarchy -->
    <Card.Root>
      <Card.Header>
        <Card.Title>Hierarchy</Card.Title>
        <Card.Description>The guardian/superior above this person, and who sits beneath them.</Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <Label>Guardian (superior)</Label>
          <Select.Root
            type="single"
            value={form.parent_id ?? NONE}
            onValueChange={(v) => (form.parent_id = v === NONE ? null : v)}
          >
            <Select.Trigger class="w-full">{personName(form.parent_id)}</Select.Trigger>
            <Select.Content>
              <Select.Item value={NONE} label="None">None</Select.Item>
              {#each parentOptions() as o (o.id)}
                <Select.Item value={o.id} label={o.name}>{o.name}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>
        <div class="space-y-1.5">
          <Label class="text-muted-foreground">Subordinates</Label>
          {#if subordinates().length === 0}
            <p class="text-sm text-muted-foreground">No one reports to this person yet.</p>
          {:else}
            <div class="flex flex-wrap gap-1.5">
              {#each subordinates() as s (s.id)}
                <button
                  class="interactive rounded-md border border-border bg-card px-2 py-1 text-xs hover:bg-primary/5"
                  onclick={() => goto(`/people/${s.id}`)}
                >
                  {s.name}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </Card.Content>
    </Card.Root>

    <!-- Recipient -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><Radio class="size-4 text-muted-foreground" /> Recipient</Card.Title>
        <Card.Description>This person's own notification target (their device).</Card.Description>
      </Card.Header>
      <Card.Content>
        <Select.Root
          type="single"
          value={form.recipient_label ?? NONE}
          onValueChange={(v) => (form.recipient_label = v === NONE ? null : v)}
        >
          <Select.Trigger class="w-full">{form.recipient_label ?? "None"}</Select.Trigger>
          <Select.Content>
            <Select.Item value={NONE} label="None">None</Select.Item>
            {#each recipients as r (r.chat_id)}
              <Select.Item value={r.label} label={r.label}>{r.label}</Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
        {#if recipients.length === 0}
          <p class="mt-2 text-xs text-muted-foreground">No recipients paired yet — add one under Recipients.</p>
        {/if}
      </Card.Content>
    </Card.Root>

    <!-- Permissions -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><ShieldCheck class="size-4 text-muted-foreground" /> Permissions over subordinates</Card.Title>
        <Card.Description>What this person may do for the people beneath them.</Card.Description>
      </Card.Header>
      <Card.Content>
        <div class="grid gap-2 sm:grid-cols-2">
          {#each permissionKinds as perm (perm)}
            <label class="flex items-center gap-2 rounded-md border border-border p-2.5 text-sm">
              <Switch checked={hasPerm(perm)} onCheckedChange={(v) => togglePerm(perm, v)} />
              <span>{permLabel(perm)}</span>
            </label>
          {/each}
        </div>
      </Card.Content>
    </Card.Root>

    <!-- Biometrics -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><ScanFace class="size-4 text-muted-foreground" /> Biometrics</Card.Title>
        <Card.Description>Face enrollment for recognition at entrances/exits. We store only the embedding, never the photo.</Card.Description>
      </Card.Header>
      <Card.Content class="flex items-center justify-between gap-4">
        <span class="flex items-center gap-2 text-sm text-muted-foreground">
          <StatusDot state={approvedFaces > 0 ? "online" : "offline"} />
          {#if isNew}
            Save this person first to enroll.
          {:else if approvedFaces > 0}
            {approvedFaces} face{approvedFaces === 1 ? "" : "s"} enrolled ({embeddings.length} total)
          {:else}
            No faces enrolled yet
          {/if}
        </span>
        {#if !isNew}
          <Button variant="outline" onclick={() => goto(`/people/${id}/enroll`)}>
            <ScanFace class="size-4" /> Open enrollment
          </Button>
        {/if}
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<!-- Delete confirmation -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Remove person?</Dialog.Title>
      <Dialog.Description>
        Remove <span class="font-medium text-foreground">{form.name}</span>. Anyone whose guardian is
        this person will be detached. This can't be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>Cancel</Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>
        {deleting ? "Removing…" : "Remove"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
