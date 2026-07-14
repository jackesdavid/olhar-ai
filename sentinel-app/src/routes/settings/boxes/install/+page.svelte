<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Copy from "@lucide/svelte/icons/copy";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Check from "@lucide/svelte/icons/check";
  import Terminal from "@lucide/svelte/icons/terminal";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import { boxes as boxApi } from "$lib/api";
  import { pendingInstall } from "$lib/box-install.svelte";

  const install = $derived(pendingInstall.value);

  let code = $state("");
  let expiresAt = $state("");
  let minting = $state(true);
  let online = $state(false);
  let showAdvanced = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;

  // The token only exists in memory from the create step — on a refresh or direct hit
  // there's nothing to show, so send the user back to the list.
  onMount(() => {
    if (!pendingInstall.value) {
      goto("/settings/boxes");
      return;
    }
    mint();
    // Watch the box arrive. The appliance checks in within seconds of being paired, and seeing it
    // happen is the whole confirmation the user needs — better than being told to go look.
    timer = setInterval(checkOnline, 4000);
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  async function mint() {
    if (!pendingInstall.value) return;
    minting = true;
    try {
      const r = await boxApi.pairingCode(pendingInstall.value.id);
      code = r.code;
      expiresAt = r.expires_at;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Couldn't create a pairing code");
    } finally {
      minting = false;
    }
  }

  async function checkOnline() {
    if (!pendingInstall.value || online) return;
    try {
      const list = await boxApi.list();
      const me = list.find((b) => b.id === pendingInstall.value?.id);
      if (me?.last_seen) {
        online = true;
        if (timer) clearInterval(timer);
      }
    } catch {
      // A failed poll is not worth a toast — the next one is four seconds away.
    }
  }

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

  function codeDisplay(): string {
    if (minting) return "·····";
    return code || "—";
  }

  function expiryHint(): string {
    if (!expiresAt) return "";
    const mins = Math.max(0, Math.round((new Date(expiresAt).getTime() - Date.now()) / 60000));
    if (mins <= 0) return "This code has expired — generate a new one.";
    return `Expires in ${mins} minute${mins === 1 ? "" : "s"}.`;
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
          <h1 class="text-2xl font-semibold tracking-tight">Connect "{install.name}"</h1>
          <p class="text-sm text-muted-foreground">
            Install Sentinel on the on-site machine, then enter this code there.
          </p>
        </div>
      </div>
    </div>

    {#if online}
      <Card.Root class="border-primary/30 bg-primary/5">
        <Card.Content class="flex items-center gap-3 py-6">
          <Check class="size-5 shrink-0 text-primary" />
          <div>
            <p class="font-medium">"{install.name}" is online.</p>
            <p class="text-sm text-muted-foreground">The appliance checked in. You can add its cameras now.</p>
          </div>
        </Card.Content>
      </Card.Root>
    {:else}
      <Card.Root>
        <Card.Header>
          <Card.Title>Pairing code</Card.Title>
          <Card.Description>
            When the installer finishes, it opens a page on the appliance asking for this code.
          </Card.Description>
        </Card.Header>
        <Card.Content class="space-y-4">
          <button
            type="button"
            class="interactive w-full rounded-lg border border-border bg-well px-4 py-6 text-center font-mono text-3xl font-semibold tracking-[0.2em] tabular-nums"
            title="Copy code"
            onclick={() => copy(code)}
            disabled={minting || !code}
          >
            {codeDisplay()}
          </button>
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{expiryHint()}</span>
            <Button variant="ghost" size="sm" onclick={mint} disabled={minting}>
              <RefreshCw class="size-3.5" /> New code
            </Button>
          </div>
          <p class="flex items-center gap-2 text-sm text-muted-foreground">
            <span class="status-live size-2 shrink-0 rounded-full bg-muted-foreground/60"></span>
            Waiting for "{install.name}" to check in…
          </p>
        </Card.Content>
      </Card.Root>
    {/if}

    <Separator />

    <div class="space-y-3">
      <Button variant="ghost" size="sm" class="-ml-2 text-muted-foreground" onclick={() => (showAdvanced = !showAdvanced)}>
        <Terminal class="size-4" /> {showAdvanced ? "Hide" : "Show"} the headless install command
      </Button>

      {#if showAdvanced}
        <Card.Root>
          <Card.Header>
            <Card.Title class="text-base">Headless install</Card.Title>
            <Card.Description>
              For a machine with no screen: this enrolls the box directly, so no pairing code is needed.
              It carries the box token — treat it like a password, and note it's shown only once.
            </Card.Description>
          </Card.Header>
          <Card.Content class="space-y-4">
            <div class="relative">
              <pre class="overflow-x-auto rounded-md border border-border bg-well p-4 pr-12 font-mono text-xs leading-relaxed">{install.command}</pre>
              <Button variant="ghost" size="icon" class="absolute right-2 top-2" title="Copy command" onclick={() => copy(install.command)}>
                <Copy class="size-4" />
              </Button>
            </div>

            <div class="space-y-1.5">
              <Label>Box token</Label>
              <div class="flex items-center gap-2">
                <code class="flex-1 truncate rounded-md border border-border bg-well px-2.5 py-1.5 font-mono text-xs">{install.token}</code>
                <Button variant="outline" size="sm" onclick={() => copy(install.token)}><Copy class="size-3.5" /> Copy</Button>
              </div>
            </div>

            {#if install.auto}
              <p class="flex items-center gap-2 text-xs text-primary">
                <Check class="size-3.5 shrink-0" /> Private networking is set up automatically — just run the command.
              </p>
            {:else if install.needsKey}
              <p class="text-xs text-muted-foreground">
                Automatic enrollment isn't configured, so the command has a <code>&lt;YOUR_TAILSCALE_AUTHKEY&gt;</code>
                placeholder — replace it with a key from your tailnet before running, or the box won't join the private network.
              </p>
            {/if}
          </Card.Content>
        </Card.Root>
      {/if}
    </div>

    <div class="flex justify-end">
      <Button onclick={done}>{online ? "Done" : "I'll do this later"}</Button>
    </div>
  </div>
{/if}
