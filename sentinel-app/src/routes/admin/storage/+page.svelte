<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";

  import * as Card from "$lib/components/ui/card";
  import * as Table from "$lib/components/ui/table";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";

  import { adminBilling, type TenantStorage } from "$lib/api";
  import { auth } from "$lib/auth.svelte";

  const isMaster = auth.me?.is_master ?? false;

  let loading = $state(true);
  let rows = $state<TenantStorage[]>([]);

  // The stock a customer sees "used" is the sum of stored bytes; GB-month is the average the
  // sampler records and the invoice charges. Both are shown — a tenant recording continuously
  // shows up here in GB-month before the invoice makes it a surprise.
  const totalStoredBytes = $derived(rows.reduce((a, r) => a + r.stored_bytes, 0));
  const totalClips = $derived(rows.reduce((a, r) => a + r.clip_count, 0));
  const overageCount = $derived(rows.filter((r) => r.projected_overage > 0).length);

  function fmtGB(bytes: number): string {
    const gb = bytes / 1e9;
    if (gb === 0) return "0";
    if (gb < 0.1) return `${(bytes / 1e6).toFixed(0)} MB`;
    return `${gb.toFixed(gb < 10 ? 2 : 1)} GB`;
  }
  function money(n: number, currency: string): string {
    const locale = currency === "BRL" ? "pt-BR" : "en-US";
    return new Intl.NumberFormat(locale, { style: "currency", currency }).format(n);
  }
  function planLabel(r: TenantStorage): string {
    return r.plan_name ?? "No plan";
  }
  /** Percent of quota the stored stock uses; 0 when the plan has no cloud quota. */
  function usedPct(r: TenantStorage): number {
    if (r.included_storage_gb <= 0) return 0;
    return Math.min(100, (r.stored_bytes / 1e9 / r.included_storage_gb) * 100);
  }
  function barClass(r: TenantStorage): string {
    const pct = usedPct(r);
    if (pct >= 90) return "bg-destructive";
    if (pct >= 75) return "bg-amber-500";
    return "bg-primary";
  }
  function quotaLabel(r: TenantStorage): string {
    if (r.retention_days <= 0) return "—";
    return `${r.included_storage_gb} GB · ${r.retention_days}d`;
  }

  async function load() {
    loading = true;
    try {
      rows = await adminBilling.storage();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load storage");
    } finally {
      loading = false;
    }
  }

  // ---- purge ----------------------------------------------------------------
  // Delete a tenant's cloud footage now, across every retention tier. Irreversible, so it's
  // gated behind a confirm; only offered when the tenant actually has cloud clips.
  let purgeTarget = $state<TenantStorage | null>(null);
  let purging = $state<string | null>(null);

  function canPurge(r: TenantStorage): boolean {
    return r.clip_count > 0;
  }

  async function confirmPurge() {
    if (!purgeTarget) return;
    const t = purgeTarget;
    purging = t.tenant_id;
    purgeTarget = null;
    try {
      const res = await adminBilling.purgeStorage(t.tenant_id);
      toast.success(`Purged ${res.objects_removed} object(s) from ${t.tenant_name}.`);
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Purge failed");
    } finally {
      purging = null;
    }
  }

  onMount(() => {
    if (isMaster) load();
  });
</script>

<!-- purge confirm -->
<Dialog.Root open={purgeTarget !== null} onOpenChange={(o: boolean) => { if (!o) purgeTarget = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Purge {purgeTarget?.tenant_name}'s cloud footage?</Dialog.Title>
      <Dialog.Description>
        {purgeTarget ? `${purgeTarget.clip_count} clip(s), ${fmtGB(purgeTarget.stored_bytes)} in the bucket` : ""}
        will be permanently deleted from cloud storage. Local copies on the edge box are not
        touched. This is audited and cannot be undone.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (purgeTarget = null)}>Cancel</Button>
      <Button variant="destructive" onclick={confirmPurge}>Purge</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<div class="flex flex-col gap-6 p-6">
  <PageHeader
    title="Storage"
    description="Cloud footage stored per tenant — heaviest first, so a runaway recorder is at the top."
    icon={HardDrive}
  />

  {#if !isMaster}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      This console is for the SaaS administrator only.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <div class="grid gap-4 sm:grid-cols-3">
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Stored across all tenants</p>
        <p class="text-2xl font-semibold tabular-nums">{fmtGB(totalStoredBytes)}</p>
      </Card.Content></Card.Root>
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Cloud clips</p>
        <p class="text-2xl font-semibold tabular-nums">{totalClips}</p>
      </Card.Content></Card.Root>
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Tenants over quota</p>
        <p class="text-2xl font-semibold tabular-nums">{overageCount}</p>
      </Card.Content></Card.Root>
    </div>

    <div class="rounded-lg border border-white/10 overflow-hidden">
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <Table.Head>Tenant</Table.Head>
            <Table.Head>Plan</Table.Head>
            <Table.Head class="text-right">Stored now</Table.Head>
            <Table.Head class="w-40">Quota used</Table.Head>
            <Table.Head class="text-right">GB-month</Table.Head>
            <Table.Head class="text-right">Projected overage</Table.Head>
            <Table.Head class="w-12"></Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each rows as r (r.tenant_id)}
            <Table.Row>
              <Table.Cell class="font-medium">{r.tenant_name}</Table.Cell>
              <Table.Cell class="text-sm text-muted-foreground">{planLabel(r)}</Table.Cell>
              <Table.Cell class="text-right tabular-nums">{fmtGB(r.stored_bytes)}</Table.Cell>
              <Table.Cell>
                {#if r.retention_days > 0}
                  <div class="flex items-center gap-2">
                    <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                      <div class="h-full rounded-full {barClass(r)}" style="width: {usedPct(r)}%"></div>
                    </div>
                    <span class="w-24 shrink-0 text-right text-xs text-muted-foreground tabular-nums">{quotaLabel(r)}</span>
                  </div>
                {:else}
                  <span class="text-xs text-muted-foreground">no cloud plan</span>
                {/if}
              </Table.Cell>
              <Table.Cell class="text-right tabular-nums">{r.gb_month.toFixed(2)}</Table.Cell>
              <Table.Cell class="text-right tabular-nums">
                {#if r.projected_overage > 0}
                  <Badge variant="outline" class="border-amber-500/30 bg-amber-500/10 text-amber-500">
                    {money(r.projected_overage, r.currency)}
                  </Badge>
                {:else}
                  <span class="text-muted-foreground">—</span>
                {/if}
              </Table.Cell>
              <Table.Cell class="text-right">
                {#if canPurge(r)}
                  <Button
                    variant="ghost"
                    size="icon"
                    title="Purge cloud footage"
                    disabled={purging === r.tenant_id}
                    onclick={() => (purgeTarget = r)}
                  >
                    {#if purging === r.tenant_id}
                      <Busy />
                    {:else}
                      <Trash2 class="h-4 w-4 text-destructive" />
                    {/if}
                  </Button>
                {/if}
              </Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    </div>
  {/if}
</div>
