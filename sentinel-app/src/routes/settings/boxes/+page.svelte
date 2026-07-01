<script lang="ts">
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import ArrowUpCircle from "@lucide/svelte/icons/arrow-up-circle";
  import ScrollText from "@lucide/svelte/icons/scroll-text";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";

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
  import { pendingInstall } from "$lib/box-install.svelte";
  import { fmtTs } from "$lib/utils";

  const allowed = hasPerm("devices.write");

  let loading = $state(true);
  let list = $state<Box[]>([]);

  // Add form
  let name = $state("");
  let authkey = $state("");
  let creating = $state(false);

  // Delete confirm
  let deleteTarget = $state<Box | null>(null);
  let deleting = $state(false);

  // Edge versioning
  let latest = $state<string>("");
  function outdated(b: Box): boolean {
    return !!b.version && !!latest && b.version !== latest;
  }

  async function load() {
    loading = true;
    try {
      [list, latest] = await Promise.all([
        boxApi.list(),
        boxApi.latestVersion().then((r) => r.version).catch(() => ""),
      ]);
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
      const boxName = name.trim();
      const box = await boxApi.create(boxName);
      // Hand the one-time token + command to the dedicated install view (not a modal —
      // the command is long and gets copied to another machine).
      pendingInstall.value = {
        name: boxName,
        token: box.token,
        command: buildCommand(box.token, box.hostname, box.tailscale_key),
        auto: !!box.tailscale_key,
        needsKey: !box.tailscale_key && !authkey.trim(),
      };
      name = "";
      authkey = "";
      await load();
      goto("/settings/boxes/install");
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
    return fmtTs(b.last_seen);
  }

  // ---- health (Tailscale + ffmpeg, reported on the heartbeat) --------------
  function tsLabel(s: string | null): string {
    return { serving: "serving", up: "up · not serving", offline: "offline", disabled: "no tailnet" }[s ?? ""] ?? "unknown";
  }
  function tsClass(s: string | null): string {
    if (s === "serving") return "border-primary/30 bg-primary/10 text-primary";
    if (s === "up") return "border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400";
    if (s === "offline") return "border-destructive/30 bg-destructive/10 text-destructive";
    return "border-border bg-muted text-muted-foreground"; // disabled / unknown
  }
  // Boxes whose Live path can't work: tailnet offline or not serving, or ffmpeg missing.
  const unhealthy = $derived(
    list.filter((b) => b.tailscale_state === "offline" || b.tailscale_state === "up" || b.ffmpeg_ok === false),
  );
  function healthHint(b: Box): string {
    if (b.tailscale_state === "offline") return `${b.name}: offline on the tailnet — Live can't reach it (check Tailscale on the box).`;
    if (b.tailscale_state === "up") return `${b.name}: on the tailnet but not serving go2rtc yet — try Restart.`;
    if (b.ffmpeg_ok === false) return `${b.name}: ffmpeg missing — snapshots / GetCurrentFrame will fail (install ffmpeg).`;
    return "";
  }

  // ---- restart / update ----------------------------------------------------
  let restartingId = $state<string | null>(null);
  async function restartBox(b: Box) {
    restartingId = b.id;
    try {
      await boxApi.restart(b.id);
      toast.success(`Restart requested for "${b.name}"`, { description: "It relaunches and reconnects within ~30s." });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to request restart");
    } finally {
      restartingId = null;
    }
  }

  let updatingId = $state<string | null>(null);
  async function updateBox(b: Box) {
    updatingId = b.id;
    try {
      await boxApi.update(b.id);
      toast.success(`Update requested for "${b.name}"`, { description: `Updating to v${latest} — ready in ~60s.` });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to request update");
    } finally {
      updatingId = null;
    }
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
    {#if unhealthy.length > 0}
      <div class="flex items-start gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-700 dark:text-amber-400">
        <TriangleAlert class="mt-0.5 size-4 shrink-0" />
        <div class="space-y-0.5">
          {#each unhealthy as b (b.id)}<p>{healthHint(b)}</p>{/each}
        </div>
      </div>
    {/if}
    <Card.Root>
      <Card.Header><Card.Title>Your boxes</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Name</th>
              <th class="px-3 py-2 text-left font-medium">Version</th>
              <th class="px-3 py-2 text-left font-medium">Tailnet host</th>
              <th class="px-3 py-2 text-left font-medium">Health</th>
              <th class="px-3 py-2 text-left font-medium">Last seen</th>
              <th class="px-3 py-2 text-right font-medium"></th>
            </tr>
          </thead>
          <tbody>
            {#each list as b (b.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">{b.name}</td>
                <td class="px-3 py-2">
                  {#if b.version}
                    <span class="font-mono text-xs tabular-nums text-muted-foreground">v{b.version}</span>
                    {#if outdated(b)}
                      <button
                        class="interactive ml-2 inline-flex items-center gap-1 rounded-full border border-amber-500/30 bg-amber-500/10 px-2 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400 disabled:opacity-50"
                        title="Update to v{latest}"
                        disabled={updatingId === b.id}
                        onclick={() => updateBox(b)}
                      >
                        {#if updatingId === b.id}
                          <LoaderCircle class="size-3 animate-spin" /> Updating…
                        {:else}
                          <ArrowUpCircle class="size-3" /> Update → v{latest}
                        {/if}
                      </button>
                    {/if}
                  {:else}
                    <span class="text-xs text-muted-foreground">—</span>
                  {/if}
                </td>
                <td class="px-3 py-2 font-mono text-xs text-muted-foreground">
                  {b.magicdns_host ?? "—"}
                </td>
                <td class="px-3 py-2">
                  <span class="flex flex-wrap items-center gap-1">
                    {#if b.platform}
                      <span class="inline-flex items-center rounded-full border border-border bg-muted px-2 py-0.5 text-[10px] font-medium text-muted-foreground" title="Reported platform (OS · arch)">
                        {b.platform}
                      </span>
                    {/if}
                    <span class="inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-medium {tsClass(b.tailscale_state)}" title="Tailscale state — go2rtc must be 'serving' over the tailnet for Live to work">
                      {tsLabel(b.tailscale_state)}
                    </span>
                    {#if b.ffmpeg_ok === false}
                      <span class="inline-flex items-center rounded-full border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-[10px] font-medium text-destructive" title="ffmpeg missing — snapshots / GetCurrentFrame will fail">
                        no ffmpeg
                      </span>
                    {/if}
                  </span>
                </td>
                <td class="px-3 py-2">
                  {#if b.last_seen}
                    <span class="text-muted-foreground">{seen(b)}</span>
                  {:else}
                    <Badge variant="secondary" class="text-[10px]">never connected</Badge>
                  {/if}
                </td>
                <td class="px-3 py-2 text-right">
                  <Button variant="ghost" size="icon" title="View logs" onclick={() => goto(`/logs?box=${b.id}`)}>
                    <ScrollText class="size-4" />
                  </Button>
                  <Button variant="ghost" size="icon" title="Restart (reconnect tailnet + re-serve)" disabled={restartingId === b.id} onclick={() => restartBox(b)}>
                    {#if restartingId === b.id}<LoaderCircle class="size-4 animate-spin" />{:else}<RotateCcw class="size-4" />{/if}
                  </Button>
                  <Button variant="ghost" size="icon" title="Remove" onclick={() => (deleteTarget = b)}>
                    <Trash2 class="size-4" />
                  </Button>
                </td>
              </tr>
            {/each}
            {#if list.length === 0}
              <tr><td colspan={6} class="py-8 text-center text-muted-foreground">No edge boxes yet.</td></tr>
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
