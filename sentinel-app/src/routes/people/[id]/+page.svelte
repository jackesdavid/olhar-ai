<script lang="ts">
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { data } from "$lib/data";
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

  import { dict, lang } from "$lib/i18n";
  import type { Person, PersonEmbedding, NotificationTarget } from "$lib/types";

  const t = $derived(dict[lang()].personDetail);
  const c = $derived(dict[lang()].common);

  const id = $derived($page.params.id ?? "");
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
    if (!pid) return t.none;
    return people.find((p) => p.id === pid)?.name ?? t.none;
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
        data.listPeople(),
        data.listBeacons(),
        data.personPermissionKinds(),
      ]);
      if (isNew) {
        form = blankPerson();
      } else {
        const found = people.find((p) => p.id === id);
        if (!found) {
          notFound = true;
        } else {
          form = { ...found, permissions: [...found.permissions] };
          embeddings = await data.listEmbeddings(id);
        }
      }
    } catch (err) {
      console.error("Failed to load person", err);
      toast.error(t.loadFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  }

  // Reload whenever the route id changes. Under route-id keying the page no longer
  // remounts on a param-only change, so an effect (not just onMount) keeps data fresh.
  let loadedId = "";
  $effect(() => {
    if (id !== loadedId) {
      loadedId = id;
      void load();
    }
  });

  async function save() {
    if (form.name.trim().length === 0) {
      toast.error(t.nameEmpty);
      return;
    }
    saving = true;
    try {
      const person: Person = {
        ...form,
        id: form.id || crypto.randomUUID(),
        name: form.name.trim(),
      };
      await data.savePerson(person);
      toast.success(isNew ? t.added : t.updated);
      await goto("/people/directory");
    } catch (err) {
      console.error("Save failed", err);
      toast.error(t.saveFailed, { description: String(err) });
    } finally {
      saving = false;
    }
  }

  async function doDelete() {
    deleting = true;
    try {
      await data.deletePerson(form.id);
      toast.success(t.removed);
      await goto("/people/directory");
    } catch (err) {
      console.error("Delete failed", err);
      toast.error(t.deleteFailed, { description: String(err) });
    } finally {
      deleting = false;
    }
  }
</script>

<div class="mx-auto max-w-3xl space-y-6">
  <!-- top bar -->
  <div class="flex items-center justify-between gap-4">
    <div class="flex min-w-0 items-center gap-3">
      <Button variant="ghost" size="icon" title={t.backToPeople} onclick={() => goto("/people/directory")}>
        <ArrowLeft class="size-4" />
      </Button>
      <div class="min-w-0">
        <h1 class="truncate text-2xl font-semibold tracking-tight">
          {isNew ? t.newPerson : form.name || t.person}
        </h1>
        <p class="text-sm text-muted-foreground">{isNew ? t.newSubtitle : t.editSubtitle}</p>
      </div>
    </div>
    {#if !loading && !notFound}
      <div class="flex shrink-0 items-center gap-2">
        {#if !isNew}
          <Button variant="outline" onclick={() => (deleteOpen = true)}>
            <Trash2 class="size-4" /> {t.delete}
          </Button>
        {/if}
        <Button onclick={save} disabled={saving}>{saving ? t.saving : c.save}</Button>
      </div>
    {/if}
  </div>

  {#if loading}
    <Loader label={t.loading} />
  {:else if notFound}
    <Card.Root>
      <Card.Content class="py-10 text-center text-sm text-muted-foreground">
        {t.goneTitle}
        <div class="mt-4">
          <Button variant="outline" onclick={() => goto("/people/directory")}>{t.backButton}</Button>
        </div>
      </Card.Content>
    </Card.Root>
  {:else}
    <!-- Details -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><Users class="size-4 text-muted-foreground" /> {t.details}</Card.Title>
      </Card.Header>
      <Card.Content>
        <div class="space-y-2">
          <Label for="person-name">{t.name}</Label>
          <Input id="person-name" bind:value={form.name} placeholder={t.namePlaceholder} autocomplete="off" />
        </div>
      </Card.Content>
    </Card.Root>

    <!-- Hierarchy -->
    <Card.Root>
      <Card.Header>
        <Card.Title>{t.hierarchy}</Card.Title>
        <Card.Description>{t.hierarchyDescription}</Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <Label>{t.guardian}</Label>
          <Select.Root
            type="single"
            value={form.parent_id ?? NONE}
            onValueChange={(v) => (form.parent_id = v === NONE ? null : v)}
          >
            <Select.Trigger class="w-full">{personName(form.parent_id)}</Select.Trigger>
            <Select.Content>
              <Select.Item value={NONE} label={t.none}>{t.none}</Select.Item>
              {#each parentOptions() as o (o.id)}
                <Select.Item value={o.id} label={o.name}>{o.name}</Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>
        <div class="space-y-1.5">
          <Label class="text-muted-foreground">{t.subordinates}</Label>
          {#if subordinates().length === 0}
            <p class="text-sm text-muted-foreground">{t.noSubordinates}</p>
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
        <Card.Title class="flex items-center gap-2"><Radio class="size-4 text-muted-foreground" /> {t.recipient}</Card.Title>
        <Card.Description>{t.recipientDescription}</Card.Description>
      </Card.Header>
      <Card.Content>
        <Select.Root
          type="single"
          value={form.recipient_label ?? NONE}
          onValueChange={(v) => (form.recipient_label = v === NONE ? null : v)}
        >
          <Select.Trigger class="w-full">{form.recipient_label ?? t.none}</Select.Trigger>
          <Select.Content>
            <Select.Item value={NONE} label={t.none}>{t.none}</Select.Item>
            {#each recipients as r (r.chat_id)}
              <Select.Item value={r.label} label={r.label}>{r.label}</Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
        {#if recipients.length === 0}
          <p class="mt-2 text-xs text-muted-foreground">{t.noRecipients}</p>
        {/if}
      </Card.Content>
    </Card.Root>

    <!-- Permissions -->
    <Card.Root>
      <Card.Header>
        <Card.Title class="flex items-center gap-2"><ShieldCheck class="size-4 text-muted-foreground" /> {t.permissions}</Card.Title>
        <Card.Description>{t.permissionsDescription}</Card.Description>
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
        <Card.Title class="flex items-center gap-2"><ScanFace class="size-4 text-muted-foreground" /> {t.biometrics}</Card.Title>
        <Card.Description>{t.biometricsDescription}</Card.Description>
      </Card.Header>
      <Card.Content class="flex items-center justify-between gap-4">
        <span class="flex items-center gap-2 text-sm text-muted-foreground">
          <StatusDot state={approvedFaces > 0 ? "online" : "offline"} />
          {#if isNew}
            {t.saveFirst}
          {:else if approvedFaces > 0}
            {t.facesEnrolled(approvedFaces, embeddings.length)}
          {:else}
            {t.noFaces}
          {/if}
        </span>
        {#if !isNew}
          <Button variant="outline" onclick={() => goto(`/people/${id}/enroll`)}>
            <ScanFace class="size-4" /> {t.openEnrollment}
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
      <Dialog.Title>{t.removeTitle}</Dialog.Title>
      <Dialog.Description>
        {t.removeBefore}<span class="font-medium text-foreground">{form.name}</span>{t.removeAfter}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>{c.cancel}</Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>
        {deleting ? t.removing : t.remove}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
