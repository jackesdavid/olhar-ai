<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";

  import Users from "@lucide/svelte/icons/users";
  import ScanSearch from "@lucide/svelte/icons/scan-search";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  import * as Card from "$lib/components/ui/card";

  import type { Person } from "$lib/types";

  // Light stats for the dashboard cards.
  let count = $state<number | null>(null);
  let enrolled = $state<number | null>(null);

  onMount(async () => {
    try {
      const [people, ids] = await Promise.all([
        invoke<Person[]>("list_people"),
        invoke<string[]>("enrolled_person_ids"),
      ]);
      count = people.length;
      enrolled = ids.length;
    } catch {
      /* stats are best-effort */
    }
  });

  const tiles = [
    {
      title: "Directory",
      desc: "Browse, add and edit people, the hierarchy, recipients and permissions.",
      icon: Users,
      href: "/people/directory",
      stat: () => (count === null ? "" : `${count} ${count === 1 ? "person" : "people"}`),
    },
    {
      title: "Recognition",
      desc: "Probe a camera frame or photo against the enrolled people — target vs. match.",
      icon: ScanSearch,
      href: "/people/recognize",
      stat: () => (enrolled === null ? "" : `${enrolled} enrolled`),
    },
  ];
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">People</h1>
    <p class="text-sm text-muted-foreground">
      Manage the people Sentinel knows, or test who it recognises.
    </p>
  </div>

  <div class="grid gap-4 sm:grid-cols-2">
    {#each tiles as t (t.href)}
      {@const Icon = t.icon}
      <button
        class="interactive group text-left"
        onclick={() => goto(t.href)}
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
              {t.title}
              {#if t.stat()}<span class="text-xs font-normal text-muted-foreground">· {t.stat()}</span>{/if}
            </Card.Title>
            <Card.Description>{t.desc}</Card.Description>
          </Card.Header>
        </Card.Root>
      </button>
    {/each}
  </div>
</div>
