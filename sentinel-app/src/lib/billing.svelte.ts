/**
 * Soft-lock state. When a tenant's subscription lapses the control plane keeps serving
 * reads but refuses every write with `402 Payment Required`. This mirrors that so the UI
 * can say so up front instead of only failing on the next click.
 *
 * Fed from two places: `/billing/subscription` on load (proactive), and the API client's
 * 402 handler (backstop). Deliberately importless — `api.ts` imports this, not the reverse.
 *
 * The banner carries the STATUS, not a prebuilt sentence, because the right action differs by
 * status and getting that wrong has a cost we have already paid: the banner used to say "choose a
 * plan" for a `past_due` tenant, and choosing a plan starts a SECOND subscription while the first
 * keeps dunning the card that failed. A failed payment is fixed in the provider's portal; only a
 * tenant with no subscription at all should be sent to the plan grid.
 */

/** Subscription statuses that put a tenant in read-only mode. Mirrors the backend. */
export const LOCKED_STATUSES = ["expired", "past_due", "canceled"];

/** What the tenant should DO about it. `portal` = fix the card; `plan` = pick a plan. */
export type BillingAction = "portal" | "plan";

export const billingLock = $state<{
  locked: boolean;
  /** The raw status from `/billing/subscription`, or null when a 402 locked us with no status. */
  status: string | null;
  /** Set only by the 402 backstop, which has a server message but no status. */
  message: string | null;
}>({
  locked: false,
  status: null,
  message: null,
});

export function lock(message: string): void {
  billingLock.locked = true;
  billingLock.message = message;
}

export function unlock(): void {
  billingLock.locked = false;
  billingLock.status = null;
  billingLock.message = null;
}

/** Reflect a status straight from `/billing/subscription`. */
export function syncFromStatus(status: string | null | undefined): void {
  if (status && LOCKED_STATUSES.includes(status)) {
    billingLock.locked = true;
    billingLock.status = status;
    billingLock.message = null;
    return;
  }
  unlock();
}

/**
 * `past_due` means the subscription exists and a payment failed — the card is what needs fixing,
 * and sending them to the plan grid is how duplicates get created. Everything else (canceled,
 * expired, or a 402 with no status) means there is nothing to manage, so the plan grid is right.
 */
export function actionFor(status: string | null): BillingAction {
  return status === "past_due" ? "portal" : "plan";
}
