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

  import { billing, type PricedPlan, type BillingSubscription, type Invoice, type SubscribeBody } from "$lib/api";

  let loading = $state(true);
  let plans = $state<PricedPlan[]>([]);
  let sub = $state<BillingSubscription | null>(null);
  let invoices = $state<Invoice[]>([]);

  let picked = $state<PricedPlan | null>(null);
  /** What the gateway accepts, in preference order — the first is the default pick. */
  let methods = $state<SubscribeBody["billing_type"][]>([]);
  let billingType = $state<SubscribeBody["billing_type"]>("CREDIT_CARD");
  let name = $state("");
  let email = $state("");
  let cpf = $state("");
  let subscribing = $state(false);

  /** Format an amount in its own currency (BRL uses pt-BR, everything else en-US). */
  function money(n: number | null | undefined, currency: string): string {
    const locale = currency === "BRL" ? "pt-BR" : "en-US";
    return new Intl.NumberFormat(locale, { style: "currency", currency }).format(n ?? 0);
  }

  const currentPlan = $derived(plans.find((p) => p.id === sub?.plan_id) ?? null);

  function statusVariant(status: string | undefined): "default" | "secondary" | "destructive" | "outline" {
    if (status === "active") return "default";
    if (status === "trialing") return "secondary";
    if (status === "past_due" || status === "canceled" || status === "expired") return "destructive";
    return "outline";
  }

  function trialNote(): string {
    if (sub?.status !== "trialing" || !sub.ended_at) return "";
    const days = Math.ceil((new Date(sub.ended_at).getTime() - Date.now()) / 86_400_000);
    if (days < 0) return "Your trial has ended — pick a plan to continue.";
    return `Trial — ${days} day${days === 1 ? "" : "s"} left. Pick a plan to keep Sentinel after it ends.`;
  }

  async function load() {
    loading = true;
    try {
      [plans, sub, invoices, methods] = await Promise.all([
        billing.plans(),
        billing.subscription(),
        billing.invoices(),
        billing.paymentMethods(),
      ]);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to load billing");
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
    if (bt === "CREDIT_CARD") return "Card";
    if (bt === "BOLETO") return "Boleto";
    return "Pix";
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
        toast.success("Opening your payment page — your plan activates once payment clears.");
      } else {
        toast.success("Subscribed — your charge is on its way.");
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to subscribe");
    } finally {
      subscribing = false;
    }
  }

  function isCurrent(p: PricedPlan): boolean {
    return sub?.plan_id === p.id && sub?.status === "active";
  }
</script>

<div class="space-y-6">
  <PageHeader title="Billing" description="Your plan, payment, and invoices." icon={Wallet} />

  {#if loading}
    <Loader />
  {:else}
    {#if trialNote()}
      <Card.Root class="border-amber-500/40">
        <Card.Content class="py-4 text-sm">{trialNote()}</Card.Content>
      </Card.Root>
    {/if}

    <Card.Root>
      <Card.Header><Card.Title>Current plan</Card.Title></Card.Header>
      <Card.Content class="flex items-center justify-between">
        <div>
          <p class="text-lg font-semibold">{currentPlan?.name ?? "No plan"}</p>
          <p class="text-sm text-muted-foreground">
            {currentPlan ? `${money(currentPlan.amount, currentPlan.currency)}/mo · up to ${currentPlan.camera_cap} cameras` : "Choose a plan below."}
          </p>
        </div>
        {#if sub}<Badge variant={statusVariant(sub.status)}>{sub.status}</Badge>{/if}
      </Card.Content>
    </Card.Root>

    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each plans as p (p.id)}
        <Card.Root class={isCurrent(p) ? "border-foreground" : ""}>
          <Card.Header>
            <Card.Title class="flex items-center justify-between">
              {p.name}
              {#if isCurrent(p)}<Badge variant="secondary" class="text-[10px]">current</Badge>{/if}
            </Card.Title>
          </Card.Header>
          <Card.Content class="space-y-3">
            <p class="text-2xl font-semibold">{p.amount > 0 ? money(p.amount, p.currency) : "Free"}<span class="text-sm font-normal text-muted-foreground">{p.amount > 0 ? "/mo" : ""}</span></p>
            <ul class="space-y-1 text-sm text-muted-foreground">
              <li>Up to {p.camera_cap} cameras</li>
              {#if p.trial_days}<li>{p.trial_days}-day free trial</li>{/if}
            </ul>
            <Button class="w-full" variant={isCurrent(p) ? "outline" : "default"} disabled={isCurrent(p) || p.amount === 0} onclick={() => choose(p)}>
              {#if isCurrent(p)}Current plan{:else if p.amount === 0}Trial only{:else}Choose {p.name}{/if}
            </Button>
          </Card.Content>
        </Card.Root>
      {/each}
    </div>

    <Card.Root>
      <Card.Header><Card.Title>Invoices</Card.Title></Card.Header>
      <Card.Content class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead>
            <tr class="border-b border-border">
              <th class="py-2 pr-4 text-left font-medium">Period</th>
              <th class="px-3 py-2 text-left font-medium">Plan</th>
              <th class="px-3 py-2 text-left font-medium">Status</th>
              <th class="px-3 py-2 text-right font-medium">Total</th>
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
              <tr><td colspan={4}><EmptyState title="No invoices yet" description="Invoices appear here once a billing period closes." /></td></tr>
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
      <Dialog.Title>Subscribe{picked ? ` — ${picked.name}` : ""}</Dialog.Title>
      <Dialog.Description>
        {picked ? `${money(picked.amount, picked.currency)}/mo, up to ${picked.camera_cap} cameras.` : ""}
      </Dialog.Description>
    </Dialog.Header>
    <form class="space-y-3" onsubmit={confirmSubscribe}>
      <div class="space-y-1.5">
        <Label>Payment method</Label>
        {#if methods.length === 0}
          <p class="text-sm text-muted-foreground">No payment method is available right now. Please try again shortly.</p>
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
        <Label for="s-name">Full name</Label>
        <Input id="s-name" bind:value={name} placeholder="Maria Silva" />
      </div>
      <div class="space-y-1.5">
        <Label for="s-email">Email</Label>
        <Input id="s-email" type="email" bind:value={email} placeholder="maria@empresa.com.br" />
      </div>
      <div class="space-y-1.5">
        <Label for="s-cpf">CPF / CNPJ <span class="text-muted-foreground">(optional)</span></Label>
        <Input id="s-cpf" bind:value={cpf} placeholder="000.000.000-00" />
      </div>
      <Dialog.Footer>
        <Button type="submit" disabled={!canSubscribe || subscribing}>
          {#if subscribing}<Busy />{/if} Confirm subscription
        </Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
