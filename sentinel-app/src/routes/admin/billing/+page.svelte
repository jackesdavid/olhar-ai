<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Wallet from "@lucide/svelte/icons/wallet";
  import Plus from "@lucide/svelte/icons/plus";
  import Pencil from "@lucide/svelte/icons/pencil";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";

  import { adminBilling, type TenantBilling, type PlanTier, type PlanWithPrices } from "$lib/api";
  import { auth } from "$lib/auth.svelte";

  /** Currencies we sell in: Brazil and the American market. */
  const CURRENCIES = ["BRL", "USD"] as const;

  const isMaster = auth.me?.is_master ?? false;

  let loading = $state(true);
  let rows = $state<TenantBilling[]>([]);
  let plans = $state<PlanWithPrices[]>([]);
  let period = $state(currentPeriod());
  let busyTenant = $state<string | null>(null);

  let planPickerFor = $state<TenantBilling | null>(null);
  let editPlan = $state<PlanTier | null>(null);
  let editPrices = $state<Record<string, number>>({});
  let savingPlan = $state(false);

  function currentPeriod(): string {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
  }

  /** Format an amount in its own currency (BRL uses pt-BR, everything else en-US). */
  function money(n: number | null | undefined, currency: string): string {
    const locale = currency === "BRL" ? "pt-BR" : "en-US";
    return new Intl.NumberFormat(locale, { style: "currency", currency }).format(n ?? 0);
  }

  function priceOf(p: PlanWithPrices, currency: string): number {
    return p.prices.find((x) => x.currency === currency)?.amount ?? 0;
  }

  function statusVariant(status: string | null): "default" | "secondary" | "destructive" | "outline" {
    if (status === "active") return "default";
    if (status === "trialing") return "secondary";
    if (status === "past_due" || status === "canceled" || status === "expired") return "destructive";
    return "outline";
  }

  function statusLabel(status: string | null): string {
    return status ?? "no plan";
  }

  function trialInfo(row: TenantBilling): string {
    if (row.status !== "trialing" || !row.ended_at) return "";
    const days = Math.ceil((new Date(row.ended_at).getTime() - Date.now()) / 86_400_000);
    if (days < 0) return "trial ended";
    return `${days}d left`;
  }

  /** MRR per currency — never blend BRL and USD into one number. */
  const mrrByCurrency = $derived(
    CURRENCIES.map((c) => ({
      currency: c,
      total: rows
        .filter((r) => r.status === "active" && r.currency === c)
        .reduce((sum, r) => sum + (r.base_fee ?? 0), 0),
    })).filter((m) => m.total > 0),
  );

  const activeCount = $derived(rows.filter((r) => r.status === "active").length);

  async function load() {
    loading = true;
    try {
      [rows, plans] = await Promise.all([adminBilling.overview(), adminBilling.plans()]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load billing");
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (isMaster) load();
    else loading = false;
  });

  async function grantTrial(row: TenantBilling) {
    busyTenant = row.tenant_id;
    try {
      await adminBilling.grantTrial(row.tenant_id);
      toast.success(`Trial granted to ${row.tenant_name}`);
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to grant trial");
    } finally {
      busyTenant = null;
    }
  }

  async function closePeriod(row: TenantBilling) {
    busyTenant = row.tenant_id;
    try {
      const inv = await adminBilling.close(row.tenant_id, period);
      toast.success(`Closed ${period} for ${row.tenant_name}: ${money(inv.total, inv.currency)}`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to close period");
    } finally {
      busyTenant = null;
    }
  }

  async function assignPlan(row: TenantBilling, planId: string) {
    planPickerFor = null;
    busyTenant = row.tenant_id;
    try {
      await adminBilling.setPlan(row.tenant_id, planId);
      toast.success(`${row.tenant_name} → ${planId}`);
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to set plan");
    } finally {
      busyTenant = null;
    }
  }

  function openNewPlan() {
    editPlan = {
      id: "",
      name: "",
      camera_cap: 1,
      trial_days: null,
      included_gpu_sec: 0,
      included_storage_gb: 0,
      included_notifications: 0,
      overage_gpu_per_sec: 0,
      overage_storage_per_gb: 0,
      overage_notification_each: 0,
      active: true,
    };
    editPrices = { BRL: 0, USD: 0 };
  }

  function openEditPlan(p: PlanWithPrices) {
    const { prices, ...tier } = p;
    editPlan = tier;
    editPrices = Object.fromEntries(CURRENCIES.map((c) => [c, priceOf(p, c)]));
  }

  function planExists(id: string): boolean {
    return plans.some((x) => x.id === id);
  }

  async function savePlan(e: SubmitEvent) {
    e.preventDefault();
    if (!editPlan || savingPlan) return;
    if (!editPlan.id.trim() || !editPlan.name.trim()) {
      toast.error("Plan id and name are required");
      return;
    }
    savingPlan = true;
    const id = editPlan.id.trim();
    try {
      await adminBilling.upsertPlan({ ...editPlan, id });
      for (const c of CURRENCIES) {
        await adminBilling.setPrice(id, c, Number(editPrices[c]) || 0);
      }
      editPlan = null;
      await load();
      toast.success("Plan saved");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to save plan");
    } finally {
      savingPlan = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader title="Billing" description="Plans, subscriptions, and invoices across every tenant." icon={Wallet}>
    {#snippet actions()}
      {#if isMaster}
        <div class="flex items-center gap-2">
          <Input class="w-32" bind:value={period} placeholder="YYYY-MM" />
          <Button variant="outline" onclick={openNewPlan}><Plus class="size-4" /> New plan</Button>
        </div>
      {/if}
    {/snippet}
  </PageHeader>

  {#if !isMaster}
    <Card.Root><Card.Content class="py-10 text-center text-sm text-muted-foreground">
      This console is for the SaaS administrator only.
    </Card.Content></Card.Root>
  {:else if loading}
    <Loader />
  {:else}
    <div class="grid gap-4 sm:grid-cols-3">
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Tenants</p>
        <p class="text-2xl font-semibold">{rows.length}</p>
      </Card.Content></Card.Root>
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">Active subscriptions</p>
        <p class="text-2xl font-semibold">{activeCount}</p>
      </Card.Content></Card.Root>
      <Card.Root><Card.Content class="pt-6">
        <p class="text-xs text-muted-foreground">MRR (active base fees)</p>
        {#if mrrByCurrency.length === 0}
          <p class="text-2xl font-semibold">—</p>
        {:else}
          <p class="text-2xl font-semibold">
            {#each mrrByCurrency as m, i (m.currency)}{i > 0 ? " · " : ""}{money(m.total, m.currency)}{/each}
          </p>
        {/if}
      </Card.Content></Card.Root>
    </div>

    <Card.Root>
      <Card.Header><Card.Title>Tenants</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Tenant</th>
              <th class="px-3 py-2 text-left font-medium">Plan</th>
              <th class="px-3 py-2 text-left font-medium">Status</th>
              <th class="px-3 py-2 text-right font-medium">Base fee</th>
              <th class="px-3 py-2 text-right font-medium">Actions</th>
            </tr>
          </thead>
          <tbody>
            {#each rows as r (r.tenant_id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">
                  {r.tenant_name}
                  <span class="ml-1.5 font-mono text-[10px] text-muted-foreground">{r.currency}</span>
                </td>
                <td class="px-3 py-2">{r.plan_name ?? "—"}</td>
                <td class="px-3 py-2">
                  <Badge variant={statusVariant(r.status)} class="text-[10px]">{statusLabel(r.status)}</Badge>
                  {#if trialInfo(r)}<span class="ml-2 text-xs text-muted-foreground">{trialInfo(r)}</span>{/if}
                </td>
                <td class="px-3 py-2 text-right tabular-nums">{money(r.base_fee, r.currency)}</td>
                <td class="px-3 py-2">
                  <div class="flex justify-end gap-1.5">
                    <Button variant="outline" size="sm" disabled={busyTenant === r.tenant_id} onclick={() => (planPickerFor = r)}>
                      {#if busyTenant === r.tenant_id}<Busy size={14} />{/if} Change plan
                    </Button>
                    <Button variant="ghost" size="sm" disabled={busyTenant === r.tenant_id} onclick={() => grantTrial(r)}>Trial</Button>
                    <Button variant="ghost" size="sm" disabled={busyTenant === r.tenant_id} onclick={() => closePeriod(r)}>Close {period}</Button>
                  </div>
                </td>
              </tr>
            {/each}
            {#if rows.length === 0}
              <tr><td colspan={5}><EmptyState title="No tenants" description="Create a tenant to start billing." /></td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header><Card.Title>Plan catalog</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Plan</th>
              {#each CURRENCIES as c (c)}
                <th class="px-3 py-2 text-right font-medium">{c}</th>
              {/each}
              <th class="px-3 py-2 text-right font-medium">Cameras</th>
              <th class="px-3 py-2 text-right font-medium">Trial</th>
              <th class="px-3 py-2 text-left font-medium">Active</th>
              <th class="px-3 py-2 text-right font-medium"></th>
            </tr>
          </thead>
          <tbody>
            {#each plans as p (p.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-medium">{p.name} <span class="ml-1 font-mono text-xs text-muted-foreground">{p.id}</span></td>
                {#each CURRENCIES as c (c)}
                  <td class="px-3 py-2 text-right tabular-nums">{money(priceOf(p, c), c)}</td>
                {/each}
                <td class="px-3 py-2 text-right tabular-nums">{p.camera_cap}</td>
                <td class="px-3 py-2 text-right tabular-nums">{p.trial_days ? `${p.trial_days}d` : "—"}</td>
                <td class="px-3 py-2"><Badge variant={p.active ? "default" : "outline"} class="text-[10px]">{p.active ? "yes" : "no"}</Badge></td>
                <td class="px-3 py-2 text-right">
                  <Button variant="ghost" size="sm" onclick={() => openEditPlan(p)}><Pencil class="size-3.5" /> Edit</Button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<!-- Change a tenant's plan -->
<Dialog.Root open={planPickerFor !== null} onOpenChange={(o) => { if (!o) planPickerFor = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Change plan{planPickerFor ? ` — ${planPickerFor.tenant_name}` : ""}</Dialog.Title>
      <Dialog.Description>Assign a plan directly (master override, no payment).</Dialog.Description>
    </Dialog.Header>
    <div class="space-y-2">
      {#each plans as p (p.id)}
        <Button
          variant="outline"
          class="w-full justify-between"
          disabled={planPickerFor?.plan_id === p.id}
          onclick={() => planPickerFor && assignPlan(planPickerFor, p.id)}
        >
          <span>{p.name}</span>
          <span class="text-muted-foreground">
            {planPickerFor ? money(priceOf(p, planPickerFor.currency), planPickerFor.currency) : ""} · {p.camera_cap} cam
          </span>
        </Button>
      {/each}
    </div>
  </Dialog.Content>
</Dialog.Root>

<!-- Create / edit a tier + its per-currency prices -->
<Dialog.Root open={editPlan !== null} onOpenChange={(o) => { if (!o) editPlan = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{editPlan && planExists(editPlan.id) ? "Edit plan" : "New plan"}</Dialog.Title>
      <Dialog.Description>Prices are set per currency. Trial days blank = paid plan.</Dialog.Description>
    </Dialog.Header>
    {#if editPlan}
      <form class="space-y-3" onsubmit={savePlan}>
        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <Label for="p-id">Id (slug)</Label>
            <Input id="p-id" bind:value={editPlan.id} placeholder="home" disabled={planExists(editPlan.id)} />
          </div>
          <div class="space-y-1.5">
            <Label for="p-name">Name</Label>
            <Input id="p-name" bind:value={editPlan.name} placeholder="Home" />
          </div>
          {#each CURRENCIES as c (c)}
            <div class="space-y-1.5">
              <Label for={`p-price-${c}`}>Price ({c})</Label>
              <Input id={`p-price-${c}`} type="number" step="0.01" bind:value={editPrices[c]} />
            </div>
          {/each}
          <div class="space-y-1.5">
            <Label for="p-cap">Camera cap</Label>
            <Input id="p-cap" type="number" bind:value={editPlan.camera_cap} />
          </div>
          <div class="space-y-1.5">
            <Label for="p-trial">Trial days (blank = none)</Label>
            <Input id="p-trial" type="number" bind:value={editPlan.trial_days} />
          </div>
          <div class="flex items-end gap-2">
            <input id="p-active" type="checkbox" bind:checked={editPlan.active} class="size-4" />
            <Label for="p-active">Active</Label>
          </div>
        </div>
        <Dialog.Footer>
          <Button type="submit" disabled={savingPlan}>
            {#if savingPlan}<Busy />{/if} Save plan
          </Button>
        </Dialog.Footer>
      </form>
    {/if}
  </Dialog.Content>
</Dialog.Root>
