<script lang="ts">
  import { onMount } from "svelte";
  import { data, onEvent } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Save from "@lucide/svelte/icons/save";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import CheckCircle2 from "@lucide/svelte/icons/circle-check-big";
  import XCircle from "@lucide/svelte/icons/circle-x";
  import Send from "@lucide/svelte/icons/send";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import Link2 from "@lucide/svelte/icons/link-2";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button, buttonVariants } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import type { NotificationTarget, TelegramConfigState } from "$lib/types";

  let beacons = $state<NotificationTarget[]>([]);
  let config = $state<TelegramConfigState | null>(null);
  let loading = $state(true);

  // ---- pairing code edit ---------------------------------------------------
  let pairingCode = $state("");
  let savingCode = $state(false);

  // ---- per-beacon action state ---------------------------------------------
  let togglingId = $state<number | null>(null);
  let testingId = $state<number | null>(null);

  // ---- enrollment link copy ------------------------------------------------
  let copied = $state(false);

  function enrollmentLink(username: string, code: string) {
    return `https://t.me/${username}?start=${code}`;
  }

  async function copyEnrollmentLink() {
    if (!config?.bot_username || !config.pairing_code) return;
    await navigator.clipboard.writeText(enrollmentLink(config.bot_username, config.pairing_code));
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  // ---- Rename dialog -------------------------------------------------------
  let renameOpen = $state(false);
  let renameTarget = $state<NotificationTarget | null>(null);
  let renameValue = $state("");
  let renaming = $state(false);

  // ---- Delete dialog -------------------------------------------------------
  let deleteOpen = $state(false);
  let deleteTarget = $state<NotificationTarget | null>(null);
  let deleting = $state(false);

  async function loadBeacons() {
    beacons = await data.listBeacons();
  }

  async function loadConfig() {
    config = await data.getTelegramConfig();
    pairingCode = config?.pairing_code ?? "";
  }

  async function load() {
    loading = true;
    try {
      await Promise.all([loadBeacons(), loadConfig()]);
    } catch (err) {
      console.error("Failed to load beacons", err);
      toast.error("Failed to load beacons", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    const unlisten = onEvent("beacons-changed", () => loadBeacons());
    return () => {
      unlisten.then((un) => un());
    };
  });

  async function saveCode() {
    savingCode = true;
    try {
      await data.setPairingCode(pairingCode);
      toast.success("Pairing code saved.");
      await loadConfig();
    } catch (err) {
      console.error("Failed to save pairing code", err);
      toast.error("Failed to save", { description: String(err) });
    } finally {
      savingCode = false;
    }
  }

  async function testSend(b: NotificationTarget) {
    testingId = b.chat_id;
    try {
      await data.testBeacon(b.chat_id);
      toast.success(`Test sent to '${b.label}'.`);
    } catch (err) {
      console.error("Test send failed", err);
      toast.error("Test failed", { description: String(err) });
    } finally {
      testingId = null;
    }
  }

  async function toggle(b: NotificationTarget, enabled: boolean) {
    togglingId = b.chat_id;
    try {
      await data.toggleBeacon(b.chat_id, enabled);
      await loadBeacons();
    } catch (err) {
      console.error("Failed to toggle beacon", err);
      toast.error("Failed to update beacon", { description: String(err) });
      await loadBeacons();
    } finally {
      togglingId = null;
    }
  }

  function openRename(b: NotificationTarget) {
    renameTarget = b;
    renameValue = b.label;
    renameOpen = true;
  }

  async function doRename() {
    if (!renameTarget) return;
    if (renameValue.trim().length === 0) {
      toast.error("Name can't be empty.");
      return;
    }
    renaming = true;
    try {
      await data.renameBeacon(renameTarget.chat_id, renameValue.trim());
      toast.success("Recipient renamed.");
      renameOpen = false;
      renameTarget = null;
      await loadBeacons();
    } catch (err) {
      console.error("Rename failed", err);
      toast.error("Rename failed", { description: String(err) });
    } finally {
      renaming = false;
    }
  }

  function confirmDelete(b: NotificationTarget) {
    deleteTarget = b;
    deleteOpen = true;
  }

  async function doDelete() {
    if (!deleteTarget) return;
    deleting = true;
    try {
      await data.deleteBeacon(deleteTarget.chat_id);
      toast.success("Recipient removed.");
      deleteOpen = false;
      deleteTarget = null;
      await loadBeacons();
    } catch (err) {
      console.error("Delete failed", err);
      toast.error("Delete failed", { description: String(err) });
    } finally {
      deleting = false;
    }
  }

  function hasEnrollmentLink() {
    return config?.provider_configured && config.bot_username && config.pairing_code;
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Recipients</h1>
    <p class="text-sm text-muted-foreground">
      Trusted Telegram contacts that receive Sentinel's alerts.
    </p>
  </div>

  <!-- Telegram enrollment -->
  <Card.Root>
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        Telegram
        {#if config?.provider_configured}
          <Badge variant="outline" class="border-emerald-500/30 bg-emerald-500/15 text-emerald-400">
            Provider active
          </Badge>
        {:else}
          <Badge variant="outline" class="border-amber-500/30 bg-amber-500/15 text-amber-400">
            Provider not configured
          </Badge>
        {/if}
      </Card.Title>
      <Card.Description>
        {#if config?.provider_configured}
          Set a pairing code — share the enrollment link with anyone who should receive alerts.
        {:else}
          The Telegram bot is not configured yet. Ask your account administrator to set it up in
          Notification Providers.
        {/if}
      </Card.Description>
    </Card.Header>
    {#if config?.provider_configured}
      <Card.Content class="space-y-4">
        <div class="space-y-2">
          <Label for="pairing-code">Pairing code</Label>
          <div class="flex gap-2">
            <Input
              id="pairing-code"
              bind:value={pairingCode}
              placeholder="choose-a-secret"
              autocomplete="off"
              class="max-w-xs"
            />
            <Button onclick={saveCode} disabled={savingCode} size="sm">
              {#if savingCode}
                <LoaderCircle class="size-4 animate-spin" />
                Saving…
              {:else}
                <Save class="size-4" />
                Save
              {/if}
            </Button>
          </div>
          <p class="text-xs text-muted-foreground">
            Anyone who knows this code can pair their Telegram with your site.
          </p>
        </div>

        {#if config.bot_username && config.pairing_code}
          <div class="rounded-md border border-border bg-muted/40 p-3 text-sm space-y-3">
            <div class="flex items-center gap-2">
              <Link2 class="size-4 shrink-0 text-emerald-400" />
              <span class="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
                {enrollmentLink(config.bot_username, config.pairing_code)}
              </span>
              <div class="flex shrink-0 items-center gap-1">
                <Button
                  variant="outline"
                  size="sm"
                  class="h-7 gap-1 text-xs"
                  onclick={copyEnrollmentLink}
                >
                  {#if copied}
                    <Check class="size-3 text-emerald-400" />
                    Copied
                  {:else}
                    <Copy class="size-3" />
                    Copy
                  {/if}
                </Button>
                <a
                  href={enrollmentLink(config.bot_username, config.pairing_code)}
                  target="_blank"
                  rel="noopener noreferrer"
                  class={buttonVariants({ variant: "outline", size: "sm" }) + " h-7 gap-1 text-xs"}
                >
                  <ExternalLink class="size-3" />
                  Open
                </a>
              </div>
            </div>
            <p class="text-xs text-muted-foreground">
              Share this link — tapping it opens Telegram and pairs the device automatically.
            </p>
          </div>
        {/if}
      </Card.Content>
    {/if}
  </Card.Root>

  <!-- Paired beacons -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Paired recipients</Card.Title>
      <Card.Description>Contacts that paired via the bot.</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label="Loading recipients…" />
      {:else if beacons.length === 0}
        <div class="flex flex-col items-center gap-4 py-8 text-center">
          <p class="text-sm font-medium">No recipients paired</p>
          {#if hasEnrollmentLink()}
            <p class="max-w-sm text-xs text-muted-foreground">
              Share the enrollment link — tapping it opens Telegram and pairs the device in one step.
            </p>
            <div class="flex items-center gap-2">
              <Button variant="outline" size="sm" class="gap-1.5" onclick={copyEnrollmentLink}>
                {#if copied}
                  <Check class="size-4 text-emerald-400" />
                  Link copied
                {:else}
                  <Copy class="size-4" />
                  Copy enrollment link
                {/if}
              </Button>
              <a
                href={enrollmentLink(config!.bot_username!, config!.pairing_code)}
                target="_blank"
                rel="noopener noreferrer"
                class={buttonVariants({ variant: "outline", size: "sm" }) + " gap-1.5"}
              >
                <ExternalLink class="size-4" />
                Pair this device
              </a>
            </div>
          {:else if config?.provider_configured}
            <p class="text-xs text-muted-foreground">
              Set a pairing code above, then share the enrollment link.
            </p>
          {:else}
            <p class="text-xs text-muted-foreground">
              The Telegram provider needs to be configured before pairing is available.
            </p>
          {/if}
        </div>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Label</Table.Head>
              <Table.Head>Chat ID</Table.Head>
              <Table.Head class="w-28">Enabled</Table.Head>
              <Table.Head class="text-right">Actions</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each beacons as b (b.chat_id)}
              <Table.Row class="transition-colors hover:bg-primary/5">
                <Table.Cell class="font-medium">
                  <span class="flex items-center gap-2">
                    <StatusDot state={b.enabled ? "online" : "offline"} />
                    {b.label}
                  </span>
                </Table.Cell>
                <Table.Cell class="tabular-nums text-muted-foreground">{b.chat_id}</Table.Cell>
                <Table.Cell>
                  <div class="flex items-center gap-2">
                    <Switch
                      checked={b.enabled}
                      disabled={togglingId === b.chat_id}
                      onCheckedChange={(v) => toggle(b, v)}
                    />
                    {#if b.enabled}
                      <CheckCircle2 class="size-4 text-emerald-400" />
                    {:else}
                      <XCircle class="size-4 text-muted-foreground" />
                    {/if}
                  </div>
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      title="Send test message"
                      disabled={testingId === b.chat_id}
                      onclick={() => testSend(b)}
                    >
                      {#if testingId === b.chat_id}
                        <LoaderCircle class="size-4 animate-spin" />
                      {:else}
                        <Send class="size-4" />
                      {/if}
                    </Button>
                    <Button variant="ghost" size="sm" title="Rename" onclick={() => openRename(b)}>
                      <Pencil class="size-4" />
                    </Button>
                    <Button variant="ghost" size="sm" title="Delete" onclick={() => confirmDelete(b)}>
                      <Trash2 class="size-4" />
                    </Button>
                  </div>
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </Card.Content>
  </Card.Root>
</div>

<!-- Rename dialog -->
<Dialog.Root bind:open={renameOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Rename recipient</Dialog.Title>
      <Dialog.Description>Give this device a recognizable name.</Dialog.Description>
    </Dialog.Header>
    <div class="space-y-2">
      <Label for="rename-label">Label</Label>
      <Input id="rename-label" bind:value={renameValue} placeholder="My phone" />
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (renameOpen = false)} disabled={renaming}>
        Cancel
      </Button>
      <Button onclick={doRename} disabled={renaming}>
        {renaming ? "Saving…" : "Save"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete dialog -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Remove recipient?</Dialog.Title>
      <Dialog.Description>
        Remove
        <span class="font-medium text-foreground">{deleteTarget?.label}</span>. It will stop
        receiving alerts until it pairs again.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>
        Cancel
      </Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>
        {deleting ? "Removing…" : "Remove"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
