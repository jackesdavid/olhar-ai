<script lang="ts">
  import { onMount } from "svelte";
  import { data, onEvent } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import BellRing from "@lucide/svelte/icons/bell-ring";
  import Send from "@lucide/svelte/icons/send";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import * as Select from "$lib/components/ui/select";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";

  import type { Alert, AlertLevel, AlertStats } from "$lib/types";
  import { fmtTs } from "$lib/utils";

  let alerts = $state<Alert[]>([]);
  let stats = $state<AlertStats | null>(null);
  let loading = $state(true);

  // ---- New alert dialog ----------------------------------------------------
  const levelOptions: { value: AlertLevel; label: string }[] = [
    { value: "debug", label: "Debug" },
    { value: "info", label: "Info" },
    { value: "warn", label: "Warn" },
    { value: "error", label: "Error" },
  ];
  let createOpen = $state(false);
  let formLevel = $state<AlertLevel>("info");
  let formMessage = $state("");
  let formRoom = $state("");
  let formSource = $state("");
  let creating = $state(false);

  const formLevelLabel = $derived(
    levelOptions.find((o) => o.value === formLevel)?.label ?? "Info",
  );

  // ---- Delete confirm dialog ----------------------------------------------
  let deleteOpen = $state(false);
  let deleteTarget = $state<Alert | null>(null);
  let deleting = $state(false);

  // ---- Deliver -------------------------------------------------------------
  let deliveringId = $state<number | null>(null);

  // Tailwind text colors per level (Debug/Info/Warn/Error).
  function levelTextClass(level: AlertLevel): string {
    switch (level) {
      case "debug":
        return "text-muted-foreground";
      case "info":
        return "text-sky-400";
      case "warn":
        return "text-amber-400";
      case "error":
        return "text-red-400";
    }
  }

  // Badge classes per level for the table.
  function levelBadgeClass(level: AlertLevel): string {
    switch (level) {
      case "debug":
        return "bg-muted text-muted-foreground border-transparent";
      case "info":
        return "bg-sky-500/15 text-sky-400 border-sky-500/30";
      case "warn":
        return "bg-amber-500/15 text-amber-400 border-amber-500/30";
      case "error":
        return "bg-red-500/15 text-red-400 border-red-500/30";
    }
  }

  /** Count for a level label out of `stats.by_level` (labels are upper-case). */
  function levelCount(label: string): number {
    return stats?.by_level.find(([l]) => l === label)?.[1] ?? 0;
  }

  function dash(s: string | null | undefined): string {
    return s && s.length > 0 ? s : "—";
  }

  async function load() {
    loading = true;
    try {
      const [a, s] = await Promise.all([
        data.listAlerts(),
        data.alertStats(),
      ]);
      alerts = a;
      stats = s;
    } catch (err) {
      console.error("Failed to load alerts", err);
      toast.error("Failed to load alerts", { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    load();
    const unlisten = onEvent("alerts-changed", () => load());
    const unlistenOpen = onEvent("stream-open", () => load());
    return () => {
      unlisten.then((un) => un());
      unlistenOpen.then((un) => un());
    };
  });

  function openCreate() {
    formLevel = "info";
    formMessage = "";
    formRoom = "";
    formSource = "";
    createOpen = true;
  }

  async function doCreate() {
    if (formMessage.trim().length === 0) {
      toast.error("Enter a message first.");
      return;
    }
    creating = true;
    try {
      await data.createAlert(
        formLevel,
        formMessage.trim(),
        formRoom.trim().length > 0 ? formRoom.trim() : null,
        formSource.trim().length > 0 ? formSource.trim() : null,
      );
      toast.success("Alert created.");
      createOpen = false;
      await load();
    } catch (err) {
      console.error("Failed to create alert", err);
      toast.error("Failed to create alert", { description: String(err) });
    } finally {
      creating = false;
    }
  }

  function confirmDelete(a: Alert) {
    deleteTarget = a;
    deleteOpen = true;
  }

  async function doDelete() {
    if (!deleteTarget) return;
    deleting = true;
    try {
      await data.deleteAlert(deleteTarget.id);
      toast.success("Alert deleted.");
      deleteOpen = false;
      deleteTarget = null;
      await load();
    } catch (err) {
      console.error("Failed to delete alert", err);
      toast.error("Failed to delete alert", { description: String(err) });
    } finally {
      deleting = false;
    }
  }

  /** Whether at least one delivery of this alert succeeded. */
  function isDelivered(a: Alert): boolean {
    return a.deliveries.some((d) => d.ok);
  }

  async function doDeliver(a: Alert) {
    deliveringId = a.id;
    try {
      const msg = await data.deliverAlert(a.id);
      toast.success(msg);
      await load(); // refresh so deliveries/delivered state updates
    } catch (err) {
      console.error("Failed to deliver alert", err);
      toast.error("Delivery failed", { description: String(err) });
    } finally {
      deliveringId = null;
    }
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Alerts</h1>
      <p class="text-sm text-muted-foreground">Events and notifications from your fleet.</p>
    </div>
    <Button onclick={openCreate}>
      <Plus class="size-4" />
      New alert
    </Button>
  </div>

  <!-- Stat cards -->
  <div class="grid gap-4 sm:grid-cols-3">
    <Card.Root>
      <Card.Header class="pb-2">
        <Card.Description>Total</Card.Description>
        <Card.Title class="text-3xl tabular-nums">{stats?.total ?? 0}</Card.Title>
      </Card.Header>
    </Card.Root>
    <Card.Root>
      <Card.Header class="pb-2">
        <Card.Description>Today</Card.Description>
        <Card.Title class="text-3xl tabular-nums">{stats?.today ?? 0}</Card.Title>
      </Card.Header>
    </Card.Root>
    <Card.Root>
      <Card.Header class="pb-2">
        <Card.Description>By level</Card.Description>
      </Card.Header>
      <Card.Content class="flex items-baseline justify-between gap-2 text-sm">
        <div class="flex flex-col items-center">
          <span class="text-lg font-semibold tabular-nums {levelTextClass('debug')}">
            {levelCount("DEBUG")}
          </span>
          <span class="text-xs text-muted-foreground">Debug</span>
        </div>
        <div class="flex flex-col items-center">
          <span class="text-lg font-semibold tabular-nums {levelTextClass('info')}">
            {levelCount("INFO")}
          </span>
          <span class="text-xs text-muted-foreground">Info</span>
        </div>
        <div class="flex flex-col items-center">
          <span class="text-lg font-semibold tabular-nums {levelTextClass('warn')}">
            {levelCount("WARN")}
          </span>
          <span class="text-xs text-muted-foreground">Warn</span>
        </div>
        <div class="flex flex-col items-center">
          <span class="text-lg font-semibold tabular-nums {levelTextClass('error')}">
            {levelCount("ERROR")}
          </span>
          <span class="text-xs text-muted-foreground">Error</span>
        </div>
      </Card.Content>
    </Card.Root>
  </div>

  <!-- Alerts table -->
  <Card.Root>
    <Card.Header>
      <Card.Title>Alert log</Card.Title>
      <Card.Description>Newest first.</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label="Loading alerts…" />
      {:else if alerts.length === 0}
        <EmptyState
          title="All clear"
          description="No alerts yet. Raise one with “New alert”, or let a workflow create them automatically."
        />
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head class="w-24">Level</Table.Head>
              <Table.Head>Message</Table.Head>
              <Table.Head>Room</Table.Head>
              <Table.Head>Source</Table.Head>
              <Table.Head>Time</Table.Head>
              <Table.Head>Delivered</Table.Head>
              <Table.Head class="text-right">Actions</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each alerts as a (a.id)}
              <Table.Row>
                <Table.Cell>
                  <Badge variant="outline" class={levelBadgeClass(a.level)}>
                    {a.level.toUpperCase()}
                  </Badge>
                </Table.Cell>
                <Table.Cell class="font-medium">{a.message}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{dash(a.room_name)}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{a.source}</Table.Cell>
                <Table.Cell class="tabular-nums text-muted-foreground">
                  {fmtTs(a.created_at)}
                </Table.Cell>
                <Table.Cell>
                  {#if isDelivered(a)}
                    <Badge
                      variant="outline"
                      class="border-emerald-500/30 bg-emerald-500/15 text-emerald-400"
                    >
                      Sent
                    </Badge>
                  {:else}
                    <span class="text-xs text-muted-foreground">—</span>
                  {/if}
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      title="Deliver to recipients"
                      disabled={deliveringId === a.id}
                      onclick={() => doDeliver(a)}
                    >
                      {#if deliveringId === a.id}
                        <LoaderCircle class="size-4 animate-spin" />
                      {:else}
                        <Send class="size-4" />
                      {/if}
                    </Button>
                    <Button variant="ghost" size="sm" title="Delete" onclick={() => confirmDelete(a)}>
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

<!-- New alert dialog -->
<Dialog.Root bind:open={createOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>New alert</Dialog.Title>
      <Dialog.Description>Raise an alert into the log.</Dialog.Description>
    </Dialog.Header>

    <div class="space-y-4">
      <div class="space-y-2">
        <Label>Level</Label>
        <Select.Root type="single" bind:value={formLevel}>
          <Select.Trigger class="w-full">{formLevelLabel}</Select.Trigger>
          <Select.Content>
            {#each levelOptions as opt (opt.value)}
              <Select.Item value={opt.value} label={opt.label}>{opt.label}</Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
      </div>
      <div class="space-y-2">
        <Label for="alert-message">Message</Label>
        <Input id="alert-message" bind:value={formMessage} placeholder="Motion detected at the gate" />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="space-y-2">
          <Label for="alert-room">Room (optional)</Label>
          <Input id="alert-room" bind:value={formRoom} placeholder="garage" />
        </div>
        <div class="space-y-2">
          <Label for="alert-source">Source (optional)</Label>
          <Input id="alert-source" bind:value={formSource} placeholder="operator" />
        </div>
      </div>
    </div>

    <Dialog.Footer>
      <Button variant="outline" onclick={() => (createOpen = false)} disabled={creating}>
        Cancel
      </Button>
      <Button onclick={doCreate} disabled={creating}>
        {#if creating}
          <LoaderCircle class="size-4 animate-spin" />
          Creating…
        {:else}
          Create
        {/if}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirm dialog -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Delete alert?</Dialog.Title>
      <Dialog.Description>
        Remove
        <span class="font-medium text-foreground">{deleteTarget?.message}</span>. This can't be
        undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>
        Cancel
      </Button>
      <Button variant="destructive" onclick={doDelete} disabled={deleting}>
        {deleting ? "Deleting…" : "Delete"}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
