<script lang="ts">
  import { toast } from "svelte-sonner";

  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Copy from "@lucide/svelte/icons/copy";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Check from "@lucide/svelte/icons/check";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";

  import { boxes as boxApi, type Box } from "$lib/api";
  import { API_BASE } from "$lib/config";
  import { hasPerm } from "$lib/auth.svelte";

  const allowed = hasPerm("devices.write");

  let loading = $state(true);
  let list = $state<Box[]>([]);

  // Add form
  let name = $state("");
  let authkey = $state("");
  let creating = $state(false);

  // "Just created" — token shown ONCE + the ready-to-run install command.
  let createdOpen = $state(false);
  let createdToken = $state("");
  let createdCmd = $state("");
  let createdAuto = $state(false); // Tailscale key was auto-minted (transparent)

  // Delete confirm
  let deleteTarget = $state<Box | null>(null);
  let deleting = $state(false);

  async function load() {
    loading = true;
    try {
      list = await boxApi.list();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load boxes");
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (allowed) load();
    else loading = false;
  });

  function buildCommand(token: string, hostname: string, tsKey: string | null): string {
    // Prefer the auto-minted key (transparent enrollment); else a manual override; else a placeholder.
    const key = tsKey || authkey.trim() || "<YOUR_TAILSCALE_AUTHKEY>";
    return [
      `curl -fsSL ${API_BASE}/install.sh | sudo sh -s -- \\`,
      `  --control ${API_BASE} \\`,
      `  --box-token ${token} \\`,
      `  --authkey ${key} \\`,
      `  --hostname ${hostname}`,
    ].join("\n");
  }

  async function create(e: SubmitEvent) {
    e.preventDefault();
    if (creating || !name.trim()) return;
    creating = true;
    try {
      const box = await boxApi.create(name.trim());
      createdToken = box.token;
      createdAuto = !!box.tailscale_key;
      createdCmd = buildCommand(box.token, box.hostname, box.tailscale_key);
      createdOpen = true;
      name = "";
      authkey = "";
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to create box");
    } finally {
      creating = false;
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

  async function confirmDelete() {
    if (!deleteTarget || deleting) return;
    deleting = true;
    try {
      await boxApi.del(deleteTarget.id);
      toast.success(`Removed "${deleteTarget.name}"`);
      deleteTarget = null;
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to remove box");
    } finally {
      deleting = false;
    }
  }

  function seen(b: Box): string {
    if (!b.last_seen) return "never";
    const d = new Date(b.last_seen);
    return Number.isNaN(d.getTime()) ? b.last_seen : d.toLocaleString();
  }
</script>

<div class="space-y-6">
  <div class="flex items-center gap-3">
    <HardDrive class="size-6 text-primary" />
    <div>
      <h1 class="text-xl font-semibold tracking-tight">Edge boxes</h1>
      <p class="text-sm text-muted-foreground">
        Install a Sentinel edge appliance on-site. It pulls only this workspace's cameras and streams them privately.
      </p>
    </div>
  </div>

  {#if !allowed}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      You don't have permission to manage edge boxes.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <Card.Root>
      <Card.Header><Card.Title>Your boxes</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Name</th>
              <th class="px-3 py-2 text-left font-medium">Tailnet host</th>
              <th class="px-3 py-2 text-left font-medium">Last seen</th>
              <th class="px-3 py-2 text-right font-medium"></th>
            </tr>
          </thead>
          <tbody>
            {#each list as b (b.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">{b.name}</td>
                <td class="px-3 py-2 font-mono text-xs text-muted-foreground">
                  {b.magicdns_host ?? "—"}
                </td>
                <td class="px-3 py-2">
                  {#if b.last_seen}
                    <span class="text-muted-foreground">{seen(b)}</span>
                  {:else}
                    <Badge variant="secondary" class="text-[10px]">never connected</Badge>
                  {/if}
                </td>
                <td class="px-3 py-2 text-right">
                  <Button variant="ghost" size="icon" title="Remove" onclick={() => (deleteTarget = b)}>
                    <Trash2 class="size-4" />
                  </Button>
                </td>
              </tr>
            {/each}
            {#if list.length === 0}
              <tr><td colspan={4} class="py-8 text-center text-muted-foreground">No edge boxes yet.</td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Add an edge box</Card.Title>
        <Card.Description>
          Name it and run the generated command on the on-site machine — private networking is handled for you.
        </Card.Description>
      </Card.Header>
      <Card.Content>
        <form class="space-y-4" onsubmit={create}>
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label for="box-name">Box name</Label>
              <Input id="box-name" bind:value={name} placeholder="Front office" />
            </div>
            <div class="space-y-1.5">
              <Label for="box-key">Tailscale auth key <span class="text-muted-foreground">(only if auto-enrollment is off)</span></Label>
              <Input id="box-key" bind:value={authkey} placeholder="usually leave blank" />
            </div>
          </div>
          <Button type="submit" disabled={!name.trim() || creating}>
            {#if creating}<LoaderCircle class="size-4 animate-spin" />{:else}<Plus class="size-4" />{/if}
            Create box
          </Button>
        </form>
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<!-- Token shown once + install command -->
<Dialog.Root bind:open={createdOpen}>
  <Dialog.Content class="max-w-xl">
    <Dialog.Header>
      <Dialog.Title>Install your edge box</Dialog.Title>
      <Dialog.Description>Run this on the on-site machine. The token is shown only once.</Dialog.Description>
    </Dialog.Header>

    <div class="space-y-3">
      <div class="flex items-start gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 p-2.5 text-xs text-amber-600 dark:text-amber-400">
        <TriangleAlert class="mt-0.5 size-4 shrink-0" />
        <span>Copy this now — for security the box token can't be shown again. You can always create a new box.</span>
      </div>

      <div class="space-y-1.5">
        <Label>Install command</Label>
        <div class="relative">
          <pre class="overflow-x-auto rounded-md border border-border bg-muted/40 p-3 pr-10 font-mono text-xs leading-relaxed">{createdCmd}</pre>
          <Button variant="ghost" size="icon" class="absolute right-1.5 top-1.5" title="Copy command" onclick={() => copy(createdCmd)}>
            <Copy class="size-4" />
          </Button>
        </div>
      </div>

      <div class="space-y-1.5">
        <Label>Box token</Label>
        <div class="flex items-center gap-2">
          <code class="flex-1 truncate rounded-md border border-border bg-muted/40 px-2.5 py-1.5 font-mono text-xs">{createdToken}</code>
          <Button variant="outline" size="sm" onclick={() => copy(createdToken)}><Copy class="size-3.5" /> Copy</Button>
        </div>
      </div>

      {#if createdAuto}
        <p class="flex items-center gap-2 text-xs text-primary">
          <Check class="size-3.5" /> Private networking is set up automatically — just run the command. The box joins
          securely with no Tailscale account needed.
        </p>
      {:else if !authkey.trim()}
        <p class="text-xs text-muted-foreground">
          Automatic enrollment isn't configured, so the command has a <code>&lt;YOUR_TAILSCALE_AUTHKEY&gt;</code>
          placeholder — replace it with a key from your tailnet before running, or the box won't join the private network.
        </p>
      {/if}
    </div>

    <Dialog.Footer>
      <Button onclick={() => (createdOpen = false)}>Done</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirm -->
<Dialog.Root open={deleteTarget !== null} onOpenChange={(o) => { if (!o) deleteTarget = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Remove edge box?</Dialog.Title>
      <Dialog.Description>
        "{deleteTarget?.name}" will stop receiving camera config. Its token is revoked; reinstalling needs a new box.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteTarget = null)}>Cancel</Button>
      <Button variant="destructive" disabled={deleting} onclick={confirmDelete}>
        {#if deleting}<LoaderCircle class="size-4 animate-spin" />{/if}
        Remove
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
