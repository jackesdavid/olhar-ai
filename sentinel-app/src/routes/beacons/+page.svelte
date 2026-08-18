<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Radio from "@lucide/svelte/icons/radio";
  import { onMount } from "svelte";
  import { data, onEvent } from "$lib/data";
  import { toast } from "svelte-sonner";

  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Busy from "$lib/components/busy.svelte";
  import CheckCircle2 from "@lucide/svelte/icons/circle-check-big";
  import XCircle from "@lucide/svelte/icons/circle-x";
  import Send from "@lucide/svelte/icons/send";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import EmptyState from "$lib/components/empty-state.svelte";

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

  import { dict, lang } from "$lib/i18n";
  import type { NotificationTarget, TelegramConfigState } from "$lib/types";

  const t = $derived(dict[lang()].beacons);
  const c = $derived(dict[lang()].common);

  let beacons = $state<NotificationTarget[]>([]);
  let config = $state<TelegramConfigState | null>(null);
  let loading = $state(true);
  /** Registered phones. A phone IS a Recipient (#139) — same picker, same addressing-by-label —
   *  so it belongs on this page rather than hidden in a mobile-only screen. */
  let phones = $state<{ id: string; label: string; platform: string; enabled: boolean }[]>([]);
  let phoneRenameTarget = $state<{ id: string; label: string } | null>(null);

  // ---- per-beacon action state ---------------------------------------------
  let togglingId = $state<number | null>(null);
  let testingId = $state<number | null>(null);
  // Phones key on a uuid, beacons on a numeric chat_id, and both must be able to be mid-test at
  // once — so this is a second piece of state rather than a widened type.
  let testingPhoneId = $state<string | null>(null);

  // ---- enrollment link copy ------------------------------------------------
  let copied = $state(false);

  function enrollmentLink(username: string, code: string) {
    return `https://t.me/${username}?start=${code}`;
  }
  /** The current enrollment link, or "" when the bot/code isn't ready. */
  function currentLink(): string {
    if (!config?.bot_username || !config.pairing_code) return "";
    return enrollmentLink(config.bot_username, config.pairing_code);
  }

  async function copyEnrollmentLink() {
    const link = currentLink();
    if (!link) return;
    await navigator.clipboard.writeText(link);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  // ---- Rename dialog -------------------------------------------------------
  let renameOpen = $state(false);
  let renameTarget = $state<NotificationTarget | null>(null);
  let renameValue = $state("");
  let renaming = $state(false);

  // ---- Delete dialog -------------------------------------------------------
  // One dialog, two kinds of Recipient — exactly like the rename dialog above. Only one of the two
  // targets is ever set; `deleteLabel`/`deleteConsequence` read whichever it is.
  let deleteOpen = $state(false);
  let deleteTarget = $state<NotificationTarget | null>(null);
  let deletePhoneTarget = $state<{ id: string; label: string } | null>(null);
  let deleting = $state(false);

  function deleteLabel() {
    return deletePhoneTarget?.label ?? deleteTarget?.label ?? "";
  }

  /** Removing a phone is NOT the mirror of renaming one. A rename repoints every graph that used
   *  the old name; a delete cannot — there is no new name to point at. So the two dialogs have to
   *  promise different things. */
  function deleteConsequence() {
    return deletePhoneTarget ? t.removePhoneAfter : t.removeAfter;
  }

  async function loadPhones() {
    try {
      phones = await data.listPushDevices();
    } catch {
      phones = []; // a phone list that failed to load must not break the beacons table
    }
  }

  async function loadBeacons() {
    const before = beacons.length;
    beacons = await data.listBeacons();
    // Live feedback: when a recipient finishes pairing (the `beacons-changed` event reloads us and
    // the list grows), the user *sees* it worked — the whole point of the enrollment flow.
    if (!loading && beacons.length > before) {
      const added = beacons.length - before;
      toast.success(added === 1 ? t.onePaired : t.manyPaired(added));
    }
  }

  /** Read-only here now: the pairing code is EDITED on Integrations (#191). This screen still
   *  needs the config to build the enrollment link its empty state offers. */
  async function loadConfig() {
    config = await data.getTelegramConfig();
  }

  async function load() {
    loading = true;
    try {
      await Promise.all([loadBeacons(), loadConfig()]);
    } catch (err) {
      console.error("Failed to load beacons", err);
      toast.error(t.loadFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadPhones();
    load();
    const unlisten = onEvent("beacons-changed", () => loadBeacons());
    // Polling fallback: the `beacons-changed` SSE event isn't always delivered (cross-pod relay /
    // dropped stream), so a newly-paired recipient could sit unseen until a manual refresh. Poll the
    // list while the page is open — silent, paused when the tab is hidden. `loadBeacons` toasts when
    // the list grows, so the new pair announces itself.
    const poll = setInterval(() => {
      if (!document.hidden) void loadBeacons();
    }, 5000);
    return () => {
      clearInterval(poll);
      unlisten.then((un) => un());
    };
  });

  async function testSend(b: NotificationTarget) {
    testingId = b.chat_id;
    try {
      await data.testBeacon(b.chat_id);
      toast.success(t.testSent(b.label));
    } catch (err) {
      console.error("Test send failed", err);
      toast.error(t.testFailed, { description: String(err) });
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
      toast.error(t.toggleFailed, { description: String(err) });
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

  function startPhoneRename(p: { id: string; label: string }) {
    phoneRenameTarget = { id: p.id, label: p.label };
    renameTarget = null;
    renameValue = p.label;
    renameOpen = true;
  }

  async function doPhoneRename() {
    if (!phoneRenameTarget) return;
    if (renameValue.trim().length === 0) {
      toast.error(t.nameEmpty);
      return;
    }
    renaming = true;
    try {
      const { workflows_updated } = await data.renamePushDevice(
        phoneRenameTarget.id,
        renameValue.trim(),
      );
      // Say how many graphs followed the rename. A Recipient is addressed BY NAME, so this is the
      // difference between a rename and a rename that quietly broke your automations.
      toast.success(workflows_updated > 0 ? t.renamedAndRepointed(workflows_updated) : t.renamed);
      renameOpen = false;
      phoneRenameTarget = null;
      await loadPhones();
    } catch (err) {
      console.error("Rename failed", err);
      toast.error(t.renameFailed, { description: String(err) });
    } finally {
      renaming = false;
    }
  }

  /** One dialog, two kinds of Recipient. */
  function submitRename() {
    if (phoneRenameTarget) return doPhoneRename();
    return doRename();
  }

  async function doRename() {
    if (!renameTarget) return;
    if (renameValue.trim().length === 0) {
      toast.error(t.nameEmpty);
      return;
    }
    renaming = true;
    try {
      await data.renameBeacon(renameTarget.chat_id, renameValue.trim());
      toast.success(t.renamed);
      renameOpen = false;
      renameTarget = null;
      await loadBeacons();
    } catch (err) {
      console.error("Rename failed", err);
      toast.error(t.renameFailed, { description: String(err) });
    } finally {
      renaming = false;
    }
  }

  async function testPhone(p: { id: string; label: string }) {
    testingPhoneId = p.id;
    try {
      await data.testPushDevice(p.label);
      toast.success(t.testSent(p.label));
    } catch (err) {
      console.error("Phone test send failed", err);
      // The server's reason matters more here than anywhere else on this page: a push can be
      // accepted by us and still die at Apple, and this button is the shortest path to finding out.
      toast.error(t.testFailed, { description: String(err) });
    } finally {
      testingPhoneId = null;
    }
  }

  function confirmDelete(b: NotificationTarget) {
    deletePhoneTarget = null;
    deleteTarget = b;
    deleteOpen = true;
  }

  function confirmDeletePhone(p: { id: string; label: string }) {
    deleteTarget = null;
    deletePhoneTarget = { id: p.id, label: p.label };
    deleteOpen = true;
  }

  /** One dialog, two kinds of Recipient. */
  function submitDelete() {
    if (deletePhoneTarget) return doPhoneDelete();
    return doDelete();
  }

  async function doPhoneDelete() {
    if (!deletePhoneTarget) return;
    deleting = true;
    try {
      await data.deletePushDevice(deletePhoneTarget.id);
      toast.success(t.removed);
      deleteOpen = false;
      deletePhoneTarget = null;
      await loadPhones();
    } catch (err) {
      console.error("Phone delete failed", err);
      toast.error(t.deleteFailed, { description: String(err) });
    } finally {
      deleting = false;
    }
  }

  async function doDelete() {
    if (!deleteTarget) return;
    deleting = true;
    try {
      await data.deleteBeacon(deleteTarget.chat_id);
      toast.success(t.removed);
      deleteOpen = false;
      deleteTarget = null;
      await loadBeacons();
    } catch (err) {
      console.error("Delete failed", err);
      toast.error(t.deleteFailed, { description: String(err) });
    } finally {
      deleting = false;
    }
  }

  function hasEnrollmentLink() {
    return config?.provider_configured && config.bot_username && config.pairing_code;
  }
</script>

<div class="space-y-6">
  <PageHeader
    title={t.title}
    description={t.description}
    icon={Radio}
  />


  <!-- Paired beacons -->
  <Card.Root>
    <Card.Header>
      <Card.Title>{t.pairedTitle}</Card.Title>
      <Card.Description>{t.pairedDescription}</Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label={t.loadingRecipients} />
      {:else if beacons.length === 0}
        <EmptyState title={t.emptyTitle}>
          {#if hasEnrollmentLink()}
            <div class="flex flex-col items-center gap-3">
              <p class="max-w-sm text-xs text-muted-foreground">{t.emptyShareHint}</p>
              <div class="flex items-center gap-2">
                <Button variant="outline" size="sm" class="gap-1.5" onclick={copyEnrollmentLink}>
                  {#if copied}
                    <Check class="size-4 text-primary" />
                    {t.linkCopied}
                  {:else}
                    <Copy class="size-4" />
                    {t.copyEnrollmentLink}
                  {/if}
                </Button>
                <a
                  href={enrollmentLink(config!.bot_username!, config!.pairing_code)}
                  target="_blank"
                  rel="noopener noreferrer"
                  class={buttonVariants({ variant: "outline", size: "sm" }) + " gap-1.5"}
                >
                  <ExternalLink class="size-4" />
                  {t.pairThisDevice}
                </a>
              </div>
            </div>
          {:else}
            <!-- The pairing code and the bot now live on Integrations (#191), so an empty list has
                 to point there rather than describe a control this screen no longer has. -->
            <div class="flex flex-col items-center gap-3">
              <p class="max-w-sm text-xs text-muted-foreground">
                {config?.provider_configured ? t.emptySetCodeHint : t.emptyNoProviderHint}
              </p>
              <a
                href="/settings/integrations?tab=telegram"
                class={buttonVariants({ variant: "outline", size: "sm" }) + " gap-1.5"}
              >
                <ExternalLink class="size-4" />
                {t.openIntegrations}
              </a>
            </div>
          {/if}
        </EmptyState>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>{t.colLabel}</Table.Head>
              <Table.Head>{t.colChatId}</Table.Head>
              <Table.Head class="w-28">{t.colEnabled}</Table.Head>
              <Table.Head class="text-right">{t.colActions}</Table.Head>
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
                      <CheckCircle2 class="size-4 text-primary" />
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
                      title={t.sendTest}
                      disabled={testingId === b.chat_id}
                      onclick={() => testSend(b)}
                    >
                      {#if testingId === b.chat_id}
                        <Busy />
                      {:else}
                        <Send class="size-4" />
                      {/if}
                    </Button>
                    <Button variant="ghost" size="sm" title={t.rename} onclick={() => openRename(b)}>
                      <Pencil class="size-4" />
                    </Button>
                    <Button variant="ghost" size="sm" title={t.delete} onclick={() => confirmDelete(b)}>
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

  <!-- Phones. A registered phone is its own Recipient, addressed by the same label a workflow
       picks from a dropdown — so naming it belongs here, next to the Telegram recipients, not
       buried in iOS settings. -->
  {#if phones.length > 0}
    <Card.Root>
      <Card.Header>
        <Card.Title>{t.phonesTitle}</Card.Title>
        <Card.Description>{t.phonesDescription}</Card.Description>
      </Card.Header>
      <Card.Content>
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>{t.colLabel}</Table.Head>
              <Table.Head>{t.colPlatform}</Table.Head>
              <Table.Head class="w-28">{t.colEnabled}</Table.Head>
              <Table.Head class="text-right">{t.colActions}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each phones as p (p.id)}
              <Table.Row class="transition-colors hover:bg-primary/5">
                <Table.Cell class="font-medium">{p.label}</Table.Cell>
                <Table.Cell class="text-muted-foreground">{p.platform}</Table.Cell>
                <Table.Cell>
                  <StatusDot state={p.enabled ? "online" : "offline"} />
                </Table.Cell>
                <Table.Cell class="text-right">
                  <div class="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      title={t.sendTestPush}
                      disabled={testingPhoneId === p.id}
                      onclick={() => testPhone(p)}
                    >
                      {#if testingPhoneId === p.id}
                        <Busy />
                      {:else}
                        <Send class="size-4" />
                      {/if}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      title={t.rename}
                      onclick={() => startPhoneRename(p)}
                    >
                      <Pencil class="size-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      title={t.delete}
                      onclick={() => confirmDeletePhone(p)}
                    >
                      <Trash2 class="size-4" />
                    </Button>
                  </div>
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<!-- Rename dialog -->
<Dialog.Root bind:open={renameOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t.renameTitle}</Dialog.Title>
      <Dialog.Description>{t.renameDescription}</Dialog.Description>
    </Dialog.Header>
    <div class="space-y-2">
      <Label for="rename-label">{t.colLabel}</Label>
      <Input id="rename-label" bind:value={renameValue} placeholder={t.renamePlaceholder} />
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (renameOpen = false)} disabled={renaming}>
        {c.cancel}
      </Button>
      <Button onclick={submitRename} disabled={renaming}>
        {renaming ? t.saving : c.save}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete dialog -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t.removeTitle}</Dialog.Title>
      <Dialog.Description>
        {t.removeBefore}<span class="font-medium text-foreground">{deleteLabel()}</span>{deleteConsequence()}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)} disabled={deleting}>
        {c.cancel}
      </Button>
      <Button variant="destructive" onclick={submitDelete} disabled={deleting}>
        {deleting ? t.removing : t.remove}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
