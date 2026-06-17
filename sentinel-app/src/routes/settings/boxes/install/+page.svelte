<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Copy from "@lucide/svelte/icons/copy";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Check from "@lucide/svelte/icons/check";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { pendingInstall } from "$lib/box-install.svelte";

  const install = $derived(pendingInstall.value);

  // The token only exists in memory from the create step — on a refresh or direct hit
  // there's nothing to show, so send the user back to the list.
  onMount(() => {
    if (!pendingInstall.value) goto("/settings/boxes");
  });

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied to clipboard");
    } catch {
      toast.error("Couldn't copy — select and copy manually.");
    }
  }

  function done() {
    pendingInstall.value = null;
    goto("/settings/boxes");
  }
</script>

{#if install}
  <div class="mx-auto max-w-2xl space-y-6">
    <div class="space-y-3">
      <Button variant="ghost" size="sm" class="-ml-2 text-muted-foreground" onclick={() => goto("/settings/boxes")}>
        <ArrowLeft class="size-4" /> Edge boxes
      </Button>
      <div class="flex items-center gap-3">
        <HardDrive class="size-6 text-primary" />
        <div>
          <h1 class="text-xl font-semibold tracking-tight">Install "{install.name}"</h1>
          <p class="text-sm text-muted-foreground">
            Run this on the on-site machine. The token is shown only once.
          </p>
        </div>
      </div>
    </div>

    <div class="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-700 dark:text-amber-400">
      <TriangleAlert class="mt-0.5 size-4 shrink-0" />
      <span>Copy this now — for security the box token can't be shown again. You can always create a new box.</span>
    </div>

    <Card.Root>
      <Card.Header>
        <Card.Title>Install command</Card.Title>
        <Card.Description>Paste it into a terminal on the on-site machine and run it.</Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4">
        <div class="relative">
          <pre class="overflow-x-auto rounded-md border border-border bg-muted/40 p-4 pr-12 font-mono text-xs leading-relaxed">{install.command}</pre>
          <Button variant="ghost" size="icon" class="absolute right-2 top-2" title="Copy command" onclick={() => copy(install.command)}>
            <Copy class="size-4" />
          </Button>
        </div>

        <div class="space-y-1.5">
          <Label>Box token</Label>
          <div class="flex items-center gap-2">
            <code class="flex-1 truncate rounded-md border border-border bg-muted/40 px-2.5 py-1.5 font-mono text-xs">{install.token}</code>
            <Button variant="outline" size="sm" onclick={() => copy(install.token)}><Copy class="size-3.5" /> Copy</Button>
          </div>
        </div>

        {#if install.auto}
          <p class="flex items-center gap-2 text-xs text-primary">
            <Check class="size-3.5 shrink-0" /> Private networking is set up automatically — just run the command. The box
            joins securely with no Tailscale account needed.
          </p>
        {:else if install.needsKey}
          <p class="text-xs text-muted-foreground">
            Automatic enrollment isn't configured, so the command has a <code>&lt;YOUR_TAILSCALE_AUTHKEY&gt;</code>
            placeholder — replace it with a key from your tailnet before running, or the box won't join the private network.
          </p>
        {/if}
      </Card.Content>
    </Card.Root>

    <div class="flex justify-end">
      <Button onclick={done}>Done</Button>
    </div>
  </div>
{/if}
