/**
 * Soft-lock state. When a tenant's subscription lapses the control plane keeps serving
 * reads but refuses every write with `402 Payment Required`. This mirrors that so the UI
 * can say so up front instead of only failing on the next click.
 *
 * Fed from two places: `/billing/subscription` on load (proactive), and the API client's
 * 402 handler (backstop). Deliberately importless — `api.ts` imports this, not the reverse.
 */

/** Subscription statuses that put a tenant in read-only mode. Mirrors the backend. */
export const LOCKED_STATUSES = ["expired", "past_due", "canceled"];

export const billingLock = $state<{ locked: boolean; message: string | null }>({
  locked: false,
  message: null,
});

export function lock(message: string): void {
  billingLock.locked = true;
  billingLock.message = message;
}

export function unlock(): void {
  billingLock.locked = false;
  billingLock.message = null;
}

/** Reflect a status straight from `/billing/subscription`. */
export function syncFromStatus(status: string | null | undefined): void {
  if (status && LOCKED_STATUSES.includes(status)) {
    lock(`Your subscription is ${status} — Sentinel is read-only until you choose a plan.`);
    return;
  }
  unlock();
}
