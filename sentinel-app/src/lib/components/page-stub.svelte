<script lang="ts">
  import * as Card from "$lib/components/ui/card";
  import Construction from "@lucide/svelte/icons/construction";

  let { title, description }: { title: string; description?: string } = $props();
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">{title}</h1>
    {#if description}
      <p class="text-sm text-muted-foreground">{description}</p>
    {/if}
  </div>

  <Card.Root class="bg-muted/40">
    <Card.Content class="flex items-center gap-3 py-8 text-muted-foreground">
      <Construction class="size-5 shrink-0" />
      <span>Coming soon — migrating from the ratatui app.</span>
    </Card.Content>
  </Card.Root>
</div>
