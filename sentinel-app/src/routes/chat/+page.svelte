<script lang="ts">
  // The conversational AI assistant ran in the desktop shell (local LLM + voice). It has
  // no cloud backend yet, so this route is a placeholder kept for when that service lands.
  // It's hidden from the nav (see `CLOUD_HIDE` in the root layout) but reachable by URL.
  import Bot from "@lucide/svelte/icons/bot";
  import EmptyState from "$lib/components/empty-state.svelte";
  import { Button } from "$lib/components/ui/button";
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Assistant</h1>
    <p class="text-sm text-muted-foreground">Ask Sentinel about your cameras, alerts, and people.</p>
  </div>

  <EmptyState
    title="Coming to the web"
    description="The Sentinel assistant is being rebuilt as a cloud service. It isn't available in the web app yet — check back soon."
  >
    <Bot class="size-8 text-muted-foreground" />
    <Button href="/" variant="outline">Back to dashboard</Button>
  </EmptyState>
</div>
