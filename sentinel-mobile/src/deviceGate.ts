// Device Gate (card #176) — the phone's half of "a human approves before the graph continues".
//
// Two ways a request reaches this phone, and only one of them is reliable:
//
//  * the push, which carries the request id and the raw token in its `data` field;
//  * `GET /device-gate/pending`, which the app asks on launch and on foreground.
//
// Decision 05 established that iOS critical alerts are unobtainable and a push is best-effort —
// batched, silenced or simply lost. So the pending list is the PATH and the push is the
// optimisation, not the other way round. A request that exists only inside a notification is a
// request that quietly expires.
import { api } from "./session";
import { thisDeviceId } from "./push";

/**
 * Biometrics, resolved at CALL time rather than imported at module load.
 *
 * `expo-local-authentication` is a native module: a binary built before it was added does not
 * contain it, and a top-level `import` of a missing native module throws while the bundle is
 * evaluating — taking the whole app down, not just this screen. A gate the phone cannot confirm is
 * a degraded feature; an app that will not start is an outage.
 *
 * `null` means this build cannot confirm anyone is present. The screen says so and refuses to
 * answer, because sending a `factor` we did not actually obtain is the silent downgrade decision 02
 * exists to prevent — worse than not answering at all.
 */
type LocalAuth = typeof import("expo-local-authentication");
function localAuth(): LocalAuth | null {
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    return require("expo-local-authentication") as LocalAuth;
  } catch {
    return null;
  }
}

/** A request as the phone sees it. No token — the phone already has that from the push, and
 *  handing it back over this endpoint would make the token pointless. */
export type PendingRequest = {
  id: string;
  prompt: string;
  flow_name: string;
  created_at: string;
  expires_at: string;
};

/** What the phone actually used to confirm. Decision 02: a biometric is a property of the DEVICE,
 *  not a requirement of the product, so a passcode is a legitimate answer — but recording WHICH is
 *  the only thing that keeps the fallback from being a silent downgrade. */
export type AuthFactor = "biometric" | "passcode";

/** Everything waiting on THIS device. Empty when the phone has never registered — there is no
 *  device to scope by, so asking would be meaningless rather than merely empty. */
export async function listPending(): Promise<PendingRequest[]> {
  const deviceId = await thisDeviceId();
  if (!deviceId) return [];
  return api<PendingRequest[]>(`/device-gate/pending?device_id=${encodeURIComponent(deviceId)}`);
}

export type AnswerOutcome =
  | { ok: true; state: string; arm: string }
  /** The request is finished — answered by someone already, or expired. Not an error to shout
   *  about: it is what a duplicate push delivery looks like from here. */
  | { ok: false; settled: true }
  | { ok: false; settled: false; reason: string };

/** Settle a request. The token proves WHICH request; the session proves WHO. Neither alone. */
export async function answer(
  id: string,
  token: string,
  approved: boolean,
  factor: AuthFactor,
): Promise<AnswerOutcome> {
  try {
    const r = await api<{ state: string; arm: string }>(
      `/device-gate/${encodeURIComponent(id)}/answer`,
      { method: "POST", body: JSON.stringify({ token, approved, factor }) },
    );
    return { ok: true, ...r };
  } catch (e) {
    const msg = (e as Error).message ?? "";
    // Control answers 409 with this sentence for an already-settled request, a wrong token and an
    // expired one alike — deliberately, so the endpoint cannot be used to probe which ids exist.
    if (msg.includes("can no longer be answered")) return { ok: false, settled: true };
    return { ok: false, settled: false, reason: msg || "Could not send your answer." };
  }
}

/** Was a biometric even possible on this phone? Drives the copy, not the decision. */
export async function biometricsAvailable(): Promise<boolean> {
  const LA = localAuth();
  if (!LA) return false;
  const [hardware, enrolled] = await Promise.all([LA.hasHardwareAsync(), LA.isEnrolledAsync()]);
  return hardware && enrolled;
}

/** Can this build confirm a person at all? False on a binary predating the native module. */
export function canConfirmPresence(): boolean {
  return localAuth() !== null;
}

export type ConfirmResult =
  | { ok: true; factor: AuthFactor }
  | { ok: false; cancelled: boolean }
  /** This build has no way to check who is holding the phone. Never answer in this state. */
  | { ok: false; unavailable: true; cancelled: false };

/**
 * Confirm the person is here, and report WHICH factor did it.
 *
 * Two calls on purpose. `authenticateAsync` reports success but never says what satisfied it, so a
 * single call with the device fallback enabled would leave us writing "biometric" into an audit
 * trail when a passcode was typed — precisely the silent downgrade decision 02 exists to prevent.
 * Asking for biometrics ALONE first makes the answer knowable: if that succeeds it really was a
 * face or a finger; if the person falls back, the second call says passcode and we record passcode.
 *
 * A phone with no enrolled biometric skips straight to the passcode call, which is the whole point
 * of decision 02 — refusing would make a single-device gate permanently unanswerable.
 */
export async function confirmPresence(prompt: string): Promise<ConfirmResult> {
  const LA = localAuth();
  if (!LA) return { ok: false, unavailable: true, cancelled: false };
  if (await biometricsAvailable()) {
    const bio = await LA.authenticateAsync({
      promptMessage: prompt,
      disableDeviceFallback: true,
      cancelLabel: "Use passcode",
    });
    if (bio.success) return { ok: true, factor: "biometric" };
    // `user_cancel` is a real cancel — the person backed out and nothing should happen. Anything
    // else (a failed face, "use passcode") continues to the fallback.
    if (bio.error === "user_cancel" || bio.error === "app_cancel") {
      return { ok: false, cancelled: true };
    }
  }

  const pass = await LA.authenticateAsync({
    promptMessage: prompt,
    disableDeviceFallback: false,
  });
  if (pass.success) return { ok: true, factor: "passcode" };
  return { ok: false, cancelled: pass.error === "user_cancel" || pass.error === "app_cancel" };
}

/** Seconds until a request expires, floored at zero. The screen ticks this locally. */
export function secondsLeft(expiresAt: string, now: number = Date.now()): number {
  return Math.max(0, Math.floor((new Date(expiresAt).getTime() - now) / 1000));
}

/** "4m 12s" — the countdown, in the smallest unit that still reads as urgent. */
export function formatLeft(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}
