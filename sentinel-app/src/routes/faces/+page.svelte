<script lang="ts">
  // Faces (#119) — a rapid triage queue: unknown faces grouped into clusters, each pre-recognized,
  // and ONE action (a click or Enter) labels the whole cluster → a person. Keyboard: Enter/Y confirm ·
  // E someone else · X dismiss · J/K move. A full view, never a modal.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";
  import ScanFace from "@lucide/svelte/icons/scan-face";
  import Check from "@lucide/svelte/icons/check";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import Users from "@lucide/svelte/icons/users";
  import X from "@lucide/svelte/icons/x";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import PageHeader from "$lib/components/page-header.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Select from "$lib/components/ui/select";
  import { data } from "$lib/data";
  import type { FaceCluster, Person } from "$lib/types";

  let clusters = $state<FaceCluster[]>([]);
  let people = $state<Person[]>([]);
  let loading = $state(true);
  let active = $state(0); // keyboard-focused card
  let busyKey = $state<string | null>(null); // cluster being committed
  let naming = $state<string | null>(null); // cluster in "name new person" mode
  let nameInput = $state("");
  let picking = $state<string | null>(null); // cluster in "pick existing" mode
  let expanded = $state<string | null>(null); // cluster whose faces are under review
  let excluded = $state<Record<string, Set<string>>>({}); // per-cluster excluded face ids

  onMount(load);
  async function load() {
    loading = true;
    try {
      const [cl, pp] = await Promise.all([data.listFaceClusters(), data.listPeople()]);
      clusters = cl;
      people = pp;
      active = 0;
    } catch (e) {
      toast.error("Couldn't load faces", { description: String(e) });
    } finally {
      loading = false;
    }
  }

  function faceIdsFor(c: FaceCluster): string[] {
    const ex = excluded[c.key];
    return ex ? c.face_ids.filter((id) => !ex.has(id)) : c.face_ids;
  }

  async function commit(c: FaceCluster, target: { personId?: string; newPersonName?: string }, who: string) {
    const ids = faceIdsFor(c);
    if (!ids.length) {
      toast.error("No faces selected");
      return;
    }
    busyKey = c.key;
    try {
      const res = await data.labelFaces(ids, target);
      toast.success(`Labeled ${res.labeled} face${res.labeled === 1 ? "" : "s"} as ${who}`);
      clusters = clusters.filter((x) => x.key !== c.key);
      naming = null;
      picking = null;
      active = Math.min(active, Math.max(0, clusters.length - 1));
      if (target.newPersonName) people = await data.listPeople();
    } catch (e) {
      toast.error("Couldn't label", { description: String(e) });
    } finally {
      busyKey = null;
    }
  }

  function confirmSuggestion(c: FaceCluster) {
    if (c.suggestion) commit(c, { personId: c.suggestion.person_id }, c.suggestion.name);
    else startNaming(c);
  }
  function startNaming(c: FaceCluster) {
    naming = c.key;
    picking = null;
    nameInput = "";
  }
  function submitName(c: FaceCluster) {
    const n = nameInput.trim();
    if (n) commit(c, { newPersonName: n }, n);
  }
  function pickPerson(c: FaceCluster, personId: string) {
    commit(c, { personId }, people.find((p) => p.id === personId)?.name ?? "person");
  }
  async function dismiss(c: FaceCluster) {
    busyKey = c.key;
    try {
      await data.ignoreFaces(c.face_ids, true);
      clusters = clusters.filter((x) => x.key !== c.key);
      active = Math.min(active, Math.max(0, clusters.length - 1));
    } catch (e) {
      toast.error("Couldn't dismiss", { description: String(e) });
    } finally {
      busyKey = null;
    }
  }
  function toggleExcluded(key: string, faceId: string) {
    const cur = new Set(excluded[key] ?? []);
    if (cur.has(faceId)) cur.delete(faceId);
    else cur.add(faceId);
    excluded = { ...excluded, [key]: cur };
  }

  function onKey(e: KeyboardEvent) {
    if (naming || picking) return; // don't hijack while typing / selecting
    const c = clusters[active];
    if (!c) return;
    const k = e.key.toLowerCase();
    if (k === "j" || e.key === "ArrowDown") {
      active = Math.min(active + 1, clusters.length - 1);
    } else if (k === "k" || e.key === "ArrowUp") {
      active = Math.max(active - 1, 0);
    } else if (e.key === "Enter" || k === "y") {
      confirmSuggestion(c);
    } else if (k === "e" && c.suggestion) {
      picking = c.key;
    } else if (k === "x" || e.key === "Delete") {
      dismiss(c);
    } else {
      return;
    }
    e.preventDefault();
  }

  // suggestion.confidence is already a 0..100 percent; render as ink weight, not a coloured bar.
  function confClass(conf: number): string {
    if (conf >= 85) return "font-semibold text-foreground";
    if (conf >= 70) return "font-medium text-foreground/80";
    return "text-muted-foreground";
  }
  function excludedCount(key: string): number {
    return excluded[key]?.size ?? 0;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="space-y-6">
  <PageHeader
    title="Faces"
    description="Unknown faces, grouped by who they look like — name a whole group in one tap."
    icon={ScanFace}
  >
    {#snippet actions()}
      <Button variant="outline" onclick={load} disabled={loading}>
        <RefreshCw class="size-4 {loading ? 'animate-spin' : ''}" /> Refresh
      </Button>
      <Button variant="outline" onclick={() => goto("/people")}>
        <Users class="size-4" /> People
      </Button>
    {/snippet}
  </PageHeader>

  {#if loading}
    <Loader label="Grouping faces…" />
  {:else if clusters.length === 0}
    <EmptyState
      title="No faces to label"
      description="As your cameras see people, unknown faces gather here for one-tap naming."
    >
      {#snippet children()}
        <Button variant="outline" onclick={() => goto("/people")}><Users class="size-4" /> People</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <p class="text-sm text-muted-foreground">
      {clusters.length} group{clusters.length === 1 ? "" : "s"} to label ·
      <kbd class="rounded border border-border px-1 text-[10px]">Enter</kbd> confirm ·
      <kbd class="rounded border border-border px-1 text-[10px]">E</kbd> someone else ·
      <kbd class="rounded border border-border px-1 text-[10px]">X</kbd> dismiss ·
      <kbd class="rounded border border-border px-1 text-[10px]">J/K</kbd> move
    </p>

    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each clusters as c, i (c.key)}
        <div
          class="flex flex-col rounded-2xl border p-4 transition-colors {i === active
            ? 'border-primary bg-primary/5'
            : 'border-border bg-card'}"
          role="group"
          onmouseenter={() => (active = i)}
        >
          <!-- representative crop -->
          <div class="relative mb-3 overflow-hidden rounded-md border border-border bg-well">
            {#if c.representative.image_key}
              <img src={data.faceImageUrl(c.representative.id)} alt="face" class="aspect-square w-full object-cover" />
            {:else}
              <div class="grid aspect-square w-full place-items-center border border-dashed border-border text-muted-foreground">
                <ScanFace class="size-8" />
              </div>
            {/if}
            <span class="absolute right-1.5 top-1.5 rounded-full bg-black/55 px-2 py-0.5 text-[11px] font-medium tabular-nums text-white">
              {c.size} face{c.size === 1 ? "" : "s"}
            </span>
          </div>

          <!-- suggestion -->
          {#if c.suggestion}
            <p class="mb-2 text-sm text-muted-foreground">
              Looks like <span class={confClass(c.suggestion.confidence)}>{c.suggestion.name}</span>
              · <span class="tabular-nums">{Math.round(c.suggestion.confidence)}%</span>
            </p>
          {:else}
            <p class="mb-2 text-sm text-muted-foreground">Not recognised — name a new person</p>
          {/if}

          <!-- the ONE action -->
          {#if naming === c.key}
            <!-- svelte-ignore a11y_autofocus -->
            <form class="flex gap-1.5" onsubmit={(e) => { e.preventDefault(); submitName(c); }}>
              <Input placeholder="Name…" bind:value={nameInput} autofocus class="h-9" />
              <Button type="submit" class="press h-9" disabled={busyKey === c.key || !nameInput.trim()}>
                {#if busyKey === c.key}<Busy />{:else}<Check class="size-4" />{/if}
              </Button>
              <Button type="button" variant="ghost" class="h-9" onclick={() => (naming = null)}><X class="size-4" /></Button>
            </form>
          {:else if picking === c.key}
            <div class="flex gap-1.5">
              <Select.Root type="single" onValueChange={(v) => v && pickPerson(c, v)}>
                <Select.Trigger class="h-9 flex-1">Pick a person…</Select.Trigger>
                <Select.Content>
                  {#each people as p (p.id)}<Select.Item value={p.id} label={p.name}>{p.name}</Select.Item>{/each}
                </Select.Content>
              </Select.Root>
              <Button variant="ghost" class="h-9" onclick={() => (picking = null)}><X class="size-4" /></Button>
            </div>
          {:else}
            <div class="flex gap-1.5">
              <Button class="press h-9 min-w-0 flex-1" disabled={busyKey === c.key} onclick={() => confirmSuggestion(c)}>
                {#if busyKey === c.key}
                  <Busy />
                {:else if c.suggestion}
                  <Check class="size-4 shrink-0" /> <span class="truncate">Confirm {c.suggestion.name}</span>
                {:else}
                  <UserPlus class="size-4 shrink-0" /> Name this person
                {/if}
              </Button>
              {#if c.suggestion}
                <Button variant="outline" class="h-9" title="Someone else (E)" onclick={() => (picking = c.key)}>
                  <Users class="size-4" />
                </Button>
              {/if}
              <Button variant="ghost" class="h-9 text-muted-foreground hover:text-destructive" title="Dismiss (X)" onclick={() => dismiss(c)}>
                <X class="size-4" />
              </Button>
            </div>
          {/if}

          <!-- review faces: exclude an outlier before labeling -->
          {#if c.size > 1}
            <button
              class="mt-2 flex items-center gap-1 self-start text-[11px] text-muted-foreground hover:text-foreground"
              onclick={() => (expanded = expanded === c.key ? null : c.key)}
            >
              <ChevronDown class="size-3 transition-transform {expanded === c.key ? 'rotate-180' : ''}" />
              Review {c.size} faces{#if excludedCount(c.key)} · {excludedCount(c.key)} excluded{/if}
            </button>
            {#if expanded === c.key}
              <div class="mt-2 grid grid-cols-4 gap-1.5">
                {#each c.face_ids as fid (fid)}
                  <button
                    class="relative overflow-hidden rounded border {excluded[c.key]?.has(fid) ? 'border-dashed border-border opacity-40' : 'border-border'}"
                    title={excluded[c.key]?.has(fid) ? "Excluded — click to include" : "Click to exclude"}
                    onclick={() => toggleExcluded(c.key, fid)}
                  >
                    <img src={data.faceImageUrl(fid)} alt="face" class="aspect-square w-full object-cover" />
                  </button>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
