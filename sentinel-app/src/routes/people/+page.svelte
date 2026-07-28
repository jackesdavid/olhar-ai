<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { data } from "$lib/data";

  import Users from "@lucide/svelte/icons/users";
  import ScanSearch from "@lucide/svelte/icons/scan-search";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  import * as Card from "$lib/components/ui/card";

  import { dict, lang } from "$lib/i18n";
  import type { Person } from "$lib/types";

  const t = $derived(dict[lang()].people);

  // Light stats for the dashboard cards.
  let count = $state<number | null>(null);
  let enrolled = $state<number | null>(null);

  onMount(async () => {
    try {
      const [people, ids] = await Promise.all([
        data.listPeople(),
        data.enrolledPersonIds(),
      ]);
      count = people.length;
      enrolled = ids.length;
    } catch {
      /* stats are best-effort */
    }
  });

  const tiles = $derived([
    {
      title: t.directoryTitle,
      desc: t.directoryDesc,
      icon: Users,
      href: "/people/directory",
      stat: () => (count === null ? "" : t.peopleCount(count)),
    },
    {
      title: t.recognitionTitle,
      desc: t.recognitionDesc,
      icon: ScanSearch,
      href: "/people/recognize",
      stat: () => (enrolled === null ? "" : t.enrolledCount(enrolled)),
    },
  ]);
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={Users}
  />

  <div class="grid gap-4 sm:grid-cols-2">
    {#each tiles as tile (tile.href)}
      {@const Icon = tile.icon}
      <button
        class="interactive group text-left"
        onclick={() => goto(tile.href)}
      >
        <Card.Root class="h-full transition-colors hover:border-primary/40 hover:bg-primary/5">
          <Card.Header>
            <div class="flex items-center justify-between gap-3">
              <span class="flex size-10 items-center justify-center rounded-lg border border-border bg-card text-primary">
                <Icon class="size-5" />
              </span>
              <ChevronRight class="size-5 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
            </div>
            <Card.Title class="mt-3 flex items-center gap-2">
              {tile.title}
              {#if tile.stat()}<span class="text-xs font-normal text-muted-foreground">· {tile.stat()}</span>{/if}
            </Card.Title>
            <Card.Description>{tile.desc}</Card.Description>
          </Card.Header>
        </Card.Root>
      </button>
    {/each}
  </div>
</div>
