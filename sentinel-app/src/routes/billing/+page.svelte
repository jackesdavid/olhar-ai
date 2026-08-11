<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";
  import Loader from "$lib/components/loader.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import Wallet from "@lucide/svelte/icons/wallet";

  import * as Card from "$lib/components/ui/card";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";

  import { billing, billingPortal, type PricedPlan, type BillingSubscription, type Invoice, type SubscribeBody, type TenantStorage } from "$lib/api";
  import { dict, lang, locale } from "$lib/i18n";

  const t = $derived(dict[lang()].billing);

  let loading = $state(true);
  let plans = $state<PricedPlan[]>([]);
  let sub = $state<BillingSubscription | null>(null);
  let invoices = $state<Invoice[]>([]);
  let storage = $state<TenantStorage | null>(null);

  let picked = $state<PricedPlan | null>(null);
  /** What the gateway accepts, in preference order — the first is the default pick. */
  let methods = $state<SubscribeBody["billing_type"][]>([]);
  let billingType = $state<SubscribeBody["billing_type"]>("CREDIT_CARD");
  let name = $state("");
  let email = $state("");
  let cpf = $state("");
  let subscribing = $state(false);
  let portalBusy = $state(false);

  /** Format an amount in its own currency, in the language the user picked. */
  function money(n: number | null | undefined, currency: string): string {
    return new Intl.NumberFormat(locale(), { style: "currency", currency }).format(n ?? 0);
  }

  const currentPlan = $derived(plans.find((p) => p.id === sub?.plan_id) ?? null);

  // ---- storage this period ---------------------------------------------------
  // The recordings page shows the stock ("am I near the cap?"); billing owns the billed quantity:
  // `gb_month` (the average GB held across sampled days) and the money it projects to.
  /** Bytes → a compact "95 MB" / "1.24 GB" figure. */
  function fmtGB(bytes: number): string {
    const gb = bytes / 1e9;
    if (gb < 0.1) return `${(bytes / 1e6).toFixed(0)} MB`;
    return `${gb.toFixed(gb < 10 ? 2 : 1)} GB`;
  }
  /** Percent of quota the stored stock uses, clamped for the bar. 0 when there's no quota. */
  function usedPct(s: TenantStorage): number {
    if (s.included_storage_gb <= 0) return 0;
    return Math.min(100, (s.stored_bytes / 1e9 / s.included_storage_gb) * 100);
  }
  function barClass(s: TenantStorage): string {
    const pct = usedPct(s);
    if (pct >= 90) return "bg-destructive";
    if (pct >= 75) return "bg-amber-500";
    return "bg-primary";
  }
  /** True when the tenant's plan sells cloud recording at all (retention > 0). */
  function hasCloud(s: TenantStorage | null): boolean {
    return !!s && s.retention_days > 0;
  }
  /** Whether an overage line is due this period. */
  function hasOverage(s: TenantStorage): boolean {
    return s.projected_overage > 0;
  }

  function statusVariant(status: string | undefined): "default" | "secondary" | "destructive" | "outline" {
    if (status === "active") return "default";
    if (status === "trialing") return "secondary";
    if (status === "past_due" || status === "canceled" || status === "expired") return "destructive";
    return "outline";
  }

  function trialNote(): string {
    if (sub?.status !== "trialing" || !sub.ended_at) return "";
    const days = Math.ceil((new Date(sub.ended_at).getTime() - Date.now()) / 86_400_000);
    return days < 0 ? t.trialEnded : t.trialLeft(days);
  }

  /** The gateway's status slug, in the reader's language. */
  function statusLabel(status: string | undefined): string {
    if (status === "active") return t.statusActive;
    if (status === "trialing") return t.statusTrialing;
    if (status === "past_due") return t.statusPastDue;
    if (status === "canceled") return t.statusCanceled;
    if (status === "expired") return t.statusExpired;
    return status ?? "";
  }

  async function load() {
    loading = true;
    try {
      [plans, sub, invoices, methods, storage] = await Promise.all([
        billing.plans(),
        billing.subscription(),
        billing.invoices(),
        billing.paymentMethods(),
        billing.storage(),
      ]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.loadFailed);
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    load();
  });

  function choose(p: PricedPlan) {
    picked = p;
    billingType = methods[0] ?? "CREDIT_CARD";
  }

  /** House style: no markup logic. Pix never reaches here today — see StripeGateway::payment_methods. */
  function methodLabel(bt: SubscribeBody["billing_type"]): string {
    if (bt === "CREDIT_CARD") return t.methodCard;
    if (bt === "BOLETO") return t.methodBoleto;
    return t.methodPix;
  }

  const canSubscribe = $derived(!!picked && methods.length > 0 && name.trim() && email.trim().includes("@"));

  async function confirmSubscribe(e: SubmitEvent) {
    e.preventDefault();
    if (!picked || subscribing) return;
    subscribing = true;
    try {
      const res = await billing.subscribe({
        plan_id: picked.id,
        billing_type: billingType,
        name: name.trim(),
        email: email.trim(),
        cpf_cnpj: cpf.trim() || undefined,
      });
      picked = null;
      await load();
      if (res.payment_url) {
        window.open(res.payment_url, "_blank", "noopener");
        toast.success(t.openingPayment);
      } else {
        toast.success(t.subscribed);
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t.subscribeFailed);
    } finally {
      subscribing = false;
    }
  }

  function isCurrent(p: PricedPlan): boolean {
    return sub?.plan_id === p.id && sub?.status === "active";
  }
  /** Does this workspace already have a subscription, in any state we would not start another over?
   *
   *  `isCurrent` only recognises `active`, which is why a `past_due` tenant saw a live Subscribe
   *  button on every plan — pressing it opened a SECOND subscription at the provider while the
   *  first kept dunning the card that had failed. Subscribing is for a workspace with nothing;
   *  everything else goes through the portal. */
  function hasSubscription(): boolean {
    return !!sub && sub.status !== "canceled" && sub.status !== "expired";
  }
  function planDisabled(p: PricedPlan): boolean {
    return isCurrent(p) || p.amount === 0 || hasSubscription();
  }
  function planButtonLabel(p: PricedPlan): string {
    if (isCurrent(p)) return t.currentPlanButton;
    if (p.amount === 0) return t.trialOnly;
    if (hasSubscription()) return t.manageInstead;
    return t.choosePlan(p.name);
  }
  /** Stripe hosts the card form; we never see a card number. */
  async function openPortal() {
    if (portalBusy) return;
    portalBusy = true;
    try {
      const { url } = await billingPortal.open();
      window.location.href = url;
    } catch (err) {
      toast.error(t.portalFailed, { description: String(err) });
      portalBusy = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader title={t.title} description={t.description} icon={Wallet} />

  {#if loading}
    <Loader />
  {:else}
    {#if trialNote()}
      <Card.Root class="border-amber-500/40">
        <Card.Content class="py-4 text-sm">{trialNote()}</Card.Content>
      </Card.Root>
    {/if}

    <Card.Root>
      <Card.Header><Card.Title>{t.currentPlanTitle}</Card.Title></Card.Header>
      <Card.Content class="flex items-center justify-between">
        <div>
          <p class="text-lg font-semibold">{currentPlan?.name ?? t.noPlan}</p>
          <p class="text-sm text-muted-foreground">
            {currentPlan ? t.planSummary(money(currentPlan.amount, currentPlan.currency), currentPlan.camera_cap) : t.chooseBelow}
          </p>
        </div>
        <div class="flex items-center gap-3">
          {#if sub}<Badge variant={statusVariant(sub.status)}>{statusLabel(sub.status)}</Badge>{/if}
          <!-- The one place to fix a card, change a plan or cancel. Present whenever a subscription
               exists, not only when it has failed — a card updated before it expires is a
               `past_due` that never happens. -->
          {#if hasSubscription()}
            <Button size="sm" variant="outline" disabled={portalBusy} onclick={openPortal}>
              {t.managePayment}
            </Button>
          {/if}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header><Card.Title>{t.storageTitle}</Card.Title></Card.Header>
      <Card.Content>
        {#if hasCloud(storage) && storage}
          <div class="flex items-baseline justify-between">
            <div>
              <span class="text-2xl font-semibold tabular-nums">{fmtGB(storage.stored_bytes)}</span>
              <span class="text-sm text-muted-foreground">{t.storedOf(storage.included_storage_gb)}</span>
            </div>
            <span class="text-xs text-muted-foreground">{t.retention(storage.retention_days)}</span>
          </div>

          <div class="mt-3 h-2 w-full overflow-hidden rounded-full bg-muted">
            <div class="h-full rounded-full transition-all {barClass(storage)}" style="width: {usedPct(storage)}%"></div>
          </div>

          <div class="mt-4 grid grid-cols-2 gap-4 border-t pt-4 text-sm">
            <div>
              <p class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t.billedThisMonth}</p>
              <p class="mt-1 text-lg font-semibold tabular-nums">{fmtGB(storage.gb_month * 1e9)}<span class="text-xs font-normal text-muted-foreground">{t.gbMonthAvg}</span></p>
            </div>
            <div>
              <p class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t.overageSoFar}</p>
              {#if hasOverage(storage)}
                <p class="mt-1 text-lg font-semibold tabular-nums text-destructive">{money(storage.projected_overage, storage.currency)}</p>
              {:else}
                <p class="mt-1 text-lg font-semibold tabular-nums">{money(0, storage.currency)}<span class="text-xs font-normal text-muted-foreground">{t.withinPlan}</span></p>
              {/if}
            </div>
          </div>

          <p class="mt-3 text-xs text-muted-foreground">
            {t.storageNote(storage.clip_count)}
          </p>
        {:else}
          <p class="text-sm text-muted-foreground">{t.noCloudPlan}</p>
        {/if}
      </Card.Content>
    </Card.Root>

    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each plans as p (p.id)}
        <Card.Root class={isCurrent(p) ? "border-foreground" : ""}>
          <Card.Header>
            <Card.Title class="flex items-center justify-between">
              {p.name}
              {#if isCurrent(p)}<Badge variant="secondary" class="text-[10px]">{t.current}</Badge>{/if}
            </Card.Title>
          </Card.Header>
          <Card.Content class="space-y-3">
            <p class="text-2xl font-semibold">{p.amount > 0 ? money(p.amount, p.currency) : t.free}<span class="text-sm font-normal text-muted-foreground">{p.amount > 0 ? t.perMonth : ""}</span></p>
            <ul class="space-y-1 text-sm text-muted-foreground">
              <li>{t.upToCameras(p.camera_cap)}</li>
              {#if p.trial_days}<li>{t.trialDays(p.trial_days)}</li>{/if}
            </ul>
            <Button class="w-full" variant={isCurrent(p) ? "outline" : "default"} disabled={planDisabled(p)} onclick={() => choose(p)}>
              {planButtonLabel(p)}
            </Button>
          </Card.Content>
        </Card.Root>
      {/each}
    </div>

    <Card.Root>
      <Card.Header><Card.Title>{t.invoices}</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">{t.colPeriod}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colPlan}</th>
              <th class="px-3 py-2 text-left font-medium">{t.colStatus}</th>
              <th class="px-3 py-2 text-right font-medium">{t.colTotal}</th>
            </tr>
          </thead>
          <tbody>
            {#each invoices as inv (inv.id)}
              <tr class="border-b border-border/50 last:border-0">
                <td class="py-2 pr-4 font-mono text-xs">{inv.period}</td>
                <td class="px-3 py-2">{inv.plan_name}</td>
                <td class="px-3 py-2"><Badge variant={inv.status === "final" ? "default" : "outline"} class="text-[10px]">{inv.status}</Badge></td>
                <td class="px-3 py-2 text-right tabular-nums">{money(inv.total, inv.currency)}</td>
              </tr>
            {/each}
            {#if invoices.length === 0}
              <tr><td colspan={4}><EmptyState title={t.noInvoicesTitle} description={t.noInvoicesDescription} /></td></tr>
            {/if}
          </tbody>
        </table>
      </Card.Content>
    </Card.Root>
  {/if}
</div>

<!-- Subscribe / pay -->
<Dialog.Root open={picked !== null} onOpenChange={(o) => { if (!o) picked = null; }}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>{picked ? t.subscribeNamed(picked.name) : t.subscribe}</Dialog.Title>
      <Dialog.Description>
        {picked ? t.subscribeSummary(money(picked.amount, picked.currency), picked.camera_cap) : ""}
      </Dialog.Description>
    </Dialog.Header>
    <form class="space-y-3" onsubmit={confirmSubscribe}>
      <div class="space-y-1.5">
        <Label>{t.paymentMethod}</Label>
        {#if methods.length === 0}
          <p class="text-sm text-muted-foreground">{t.noMethods}</p>
        {:else}
          <div class="flex gap-2">
            {#each methods as bt (bt)}
              <Button type="button" variant={billingType === bt ? "default" : "outline"} size="sm" onclick={() => (billingType = bt)}>
                {methodLabel(bt)}
              </Button>
            {/each}
          </div>
        {/if}
      </div>
      <div class="space-y-1.5">
        <Label for="s-name">{t.fullName}</Label>
        <Input id="s-name" bind:value={name} placeholder={t.fullNamePlaceholder} />
      </div>
      <div class="space-y-1.5">
        <Label for="s-email">{t.email}</Label>
        <Input id="s-email" type="email" bind:value={email} placeholder={t.emailPlaceholder} />
      </div>
      <div class="space-y-1.5">
        <Label for="s-cpf">{t.taxIdLabel} <span class="text-muted-foreground">{t.optional}</span></Label>
        <Input id="s-cpf" bind:value={cpf} placeholder="000.000.000-00" />
      </div>
      <Dialog.Footer>
        <Button type="submit" disabled={!canSubscribe || subscribing}>
          {#if subscribing}<Busy />{/if} {t.confirmSubscription}
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
