<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { toast } from "svelte-sonner";

  import Plus from "@lucide/svelte/icons/plus";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import type { Person } from "$lib/types";

  let people = $state<Person[]>([]);
  let enrolledIds = $state<Set<string>>(new Set());
  let loading = $state(true);
  let query = $state("");

  // Case-insensitive filter by name. (Recipient label is searchable too.)
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return people;
    return people.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        (p.recipient_label ?? "").toLowerCase().includes(q),
    );
  });

  function permLabel(slug: string): string {
    const s = slug.replace(/_/g, " ");
    return s.charAt(0).toUpperCase() + s.slice(1);
  }
  function personName(id: string | null): string {
    if (!id) return "—";
    return people.find((p) => p.id === id)?.name ?? "—";
  }

  async function load() {
    loading = true;
    try {
      const [list, enrolled] = await Promise.all([
        invoke<Person[]>("list_people"),
        invoke<string[]>("enrolled_person_ids"),
      ]);
      people = list;
      enrolledIds = new Set(enrolled);
    } catch (err) {
      console.error("Failed to load people", err);
      toast.error("Failed to load people", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(load);
</script>

<div class="space-y-6">
  <div class="flex items-start justify-between gap-4">
    <div class="flex min-w-0 items-center gap-3">
      <Button variant="ghost" size="icon" title="Back to People" onclick={() => goto("/people")}>
        <ArrowLeft class="size-4" />
      </Button>
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">Directory</h1>
        <p class="text-sm text-muted-foreground">
          The people Sentinel manages — guardian hierarchy, notification recipient, and permissions.
        </p>
      </div>
    </div>
    <Button onclick={() => goto("/people/new")}>
      <Plus class="size-4" /> Add person
    </Button>
  </div>

  <Card.Root>
    <Card.Header>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <Card.Title>People</Card.Title>
          <Card.Description>Select a person to open their full record.</Card.Description>
        </div>
        {#if !loading && people.length > 0}
          <div class="relative w-full sm:w-64">
            <Search class="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input class="pl-8" placeholder="Search by name…" bind:value={query} />
          </div>
        {/if}
      </div>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label="Loading people…" />
      {:else if people.length === 0}
        <EmptyState
          title="No people yet"
          description="Add a person to start building the hierarchy. Face enrollment comes next."
        />
      {:else if filtered.length === 0}
        <EmptyState title="No matches" description={`No one matches “${query}”.`} />
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Name</Table.Head>
              <Table.Head>Guardian</Table.Head>
              <Table.Head>Recipient</Table.Head>
              <Table.Head>Permissions</Table.Head>
              <Table.Head class="w-24">Enrolled</Table.Head>
              <Table.Head class="w-10"></Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each filtered as p (p.id)}
              <Table.Row
                class="cursor-pointer transition-colors hover:bg-primary/5"
                onclick={() => goto(`/people/${p.id}`)}
              >
                <Table.Cell class="font-medium">{p.name}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{personName(p.parent_id)}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{p.recipient_label ?? "—"}</Table.Cell>
                <Table.Cell>
                  {#if p.permissions.length === 0}
                    <span class="text-muted-foreground">—</span>
                  {:else}
                    <div class="flex flex-wrap gap-1">
                      {#each p.permissions as perm (perm)}
                        <Badge variant="outline" class="text-[10px]">{permLabel(perm)}</Badge>
                      {/each}
                    </div>
                  {/if}
                </Table.Cell>
                <Table.Cell>
                  <span class="flex items-center gap-2 text-xs text-muted-foreground">
                    <StatusDot state={enrolledIds.has(p.id) ? "online" : "offline"} />
                    {enrolledIds.has(p.id) ? "Yes" : "No"}
                  </span>
                </Table.Cell>
                <Table.Cell class="text-right text-muted-foreground">
                  <ChevronRight class="size-4" />
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
