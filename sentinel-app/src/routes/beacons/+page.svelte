<script lang="ts">
  import { onMount } from "svelte";
  import { data, onEvent } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Save from "@lucide/svelte/icons/save";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Radio from "@lucide/svelte/icons/radio";
  import CheckCircle2 from "@lucide/svelte/icons/circle-check-big";
  import XCircle from "@lucide/svelte/icons/circle-x";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Badge } from "$lib/components/ui/badge";
  import { Separator } from "$lib/components/ui/separator";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import StatusDot from "$lib/components/status-dot.svelte";

  import type { NotificationTarget, TelegramConfigState } from "$lib/types";

  let beacons = $state<NotificationTarget[]>([]);
  let config = $state<TelegramConfigState | null>(null);
  let loading = $state(true);

  // ---- Telegram config form ------------------------------------------------
  let botToken = $state("");
  let pairingCode = $state("");
  let savingConfig = $state(false);

  // ---- per-beacon action state ---------------------------------------------
  let togglingId = $state<number | null>(null);

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
    // Live-refresh when the backend pairing poller adds a beacon.
    const unlisten = onEvent("beacons-changed", () => loadBeacons());
    return () => {
      unlisten.then((un) => un());
    };
  });

  async function saveConfig() {
    savingConfig = true;
    try {
      const msg = await data.setTelegramConfig(botToken, pairingCode);
      botToken = ""; // never keep the token in the field
      toast.success("Telegram config saved.", { description: msg });
      await loadConfig();
    } catch (err) {
      console.error("Failed to save Telegram config", err);
      toast.error("Failed to save config", { description: String(err) });
    } finally {
      savingConfig = false;
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
      await loadBeacons(); // resync the switch
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
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Recipients</h1>
    <p class="text-sm text-muted-foreground">
      Trusted Telegram contacts that receive Sentinel's alerts.
    </p>
  </div>

  <!-- Telegram config -->
  <Card.Root>
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        Telegram
        {#if config?.configured}
          <Badge variant="outline" class="border-emerald-500/30 bg-emerald-500/15 text-emerald-400">
            Configured
          </Badge>
        {:else}
          <Badge variant="outline" class="border-amber-500/30 bg-amber-500/15 text-amber-400">
            Not configured
          </Badge>
        {/if}
      </Card.Title>
      <Card.Description>
        Create a bot with @BotFather, paste its token below, and choose a secret pairing code.
      </Card.Description>
    </Card.Header>
    <Card.Content class="space-y-4">
      <div class="grid gap-4 sm:grid-cols-2">
        <div class="space-y-2">
          <Label for="bot-token">Bot token</Label>
          <Input
            id="bot-token"
            type="password"
            bind:value={botToken}
            placeholder={config?.configured ? "•••••••• (set — leave blank to keep)" : "123456:ABC-DEF…"}
            autocomplete="off"
          />
        </div>
        <div class="space-y-2">
          <Label for="pairing-code">Pairing code</Label>
          <Input id="pairing-code" bind:value={pairingCode} placeholder="choose-a-secret" autocomplete="off" />
        </div>
      </div>

      {#if config?.configured && config.pairing_code.trim().length > 0}
        <div class="rounded-md border border-border bg-muted/40 p-3 text-sm">
          <p class="text-muted-foreground">
            To pair a phone, DM the bot:
            <code class="rounded bg-background px-1.5 py-0.5 font-mono text-foreground">
              /pair {config.pairing_code}
            </code>
          </p>
          <p class="mt-1 text-xs text-muted-foreground">
            Once paired, the device appears below. Token changes take effect after an app restart.
          </p>
        </div>
      {/if}

      <div class="flex justify-end">
        <Button onclick={saveConfig} disabled={savingConfig}>
          {#if savingConfig}
            <LoaderCircle class="size-4 animate-spin" />
            Saving…
          {:else}
            <Save class="size-4" />
            Save config
          {/if}
        </Button>
      </div>
    </Card.Content>
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
        <EmptyState
          title="No recipients paired"
          description="DM the bot the pairing code above — paired phones appear here automatically."
        />
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
