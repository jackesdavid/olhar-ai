# A push Expo accepted is not a push that arrived

**Board card:** #190 Push delivery is never verified — we read the ticket, never the receipt
**Goal:** A push that dies at Apple or Google shows up as a failure with Apple's reason, in the run
log and on the device row — instead of a run that reads "asked iPhone" and a phone that stays quiet.

## The hole this fell through

Expo's send API answers in two stages, and we only read the first:

| Stage | What it means | Do we read it? |
|---|---|---|
| **Ticket** (in the POST response) | Expo queued it. `status: "ok"` says nothing about delivery. | yes — `post_expo` |
| **Receipt** (`POST /--/api/v2/push/getReceipts`, seconds later) | APNs/FCM accepted or rejected it, **with the reason**. | **no** |

On 2026-08-17 every Device Gate push was dying at Apple with

```
BadEnvironmentKeyInToken (APNs 403)
```

— the APNs `.p8` key registered with Expo was created with **Environment: Sandbox**, while a
TestFlight build registers a **production** device token. Proven by sending a deliberately invalid
device token to both Apple hosts: production answered `403 BadEnvironmentKeyInToken` (key refused),
sandbox answered `400 BadDeviceToken` (key accepted, only the fake token bad).

Every one of those runs logged `Device Gate: asked iPhone` and then `expired → failure`, which reads
exactly like *the person ignored it*. The run history said the gate was delivered and unanswered.
It was never delivered. **That is the bug worth fixing** — the key was a configuration mistake and
took ten minutes to correct once named; the silence cost far more.

## Phase 1 — read the receipt

- `post_expo` returns the ticket ids it got back instead of only a count.
- A follow-up `getReceipts` call, after a short delay, resolves those ids. Because the caller (a
  Device Gate) cannot block, this belongs on the **`work_jobs` queue** — enqueue a
  `push_receipt_check` job carrying the ticket ids, tenant and the device row, and run it a few
  seconds later via `run_after`.
- A receipt with `status: "error"` records the reason. Two of them are specific enough to act on:
  - `DeviceNotRegistered` → mark the `push_devices` row disabled; the app was uninstalled. This is
    the one case where mutating our own state is right.
  - anything else (`BadEnvironmentKeyInToken`, `MismatchSenderId`, `InvalidCredentials`) → an
    operator problem; surface it, change nothing.

## Phase 2 — say it where someone is looking

- The failing reason lands in the **run log** for the node that sent it, so the gate that "expired"
  says why nobody could answer.
- The Recipients screen shows a phone's last delivery result next to it. Paired with #187's Test
  button, that makes the whole channel provable from one screen in one click.

## Phase 3 — fail the gate honestly

A Device Gate whose push was rejected should not sit and expire. Once the receipt is known bad,
settle the request `failed` with the reason instead of waiting out the TTL — the person was never
asked, and the baseline already lists "nobody was asked" as a `failure` case.

## Verification criteria

1. With a deliberately broken APNs key, a Device Gate run logs the APNs reason within seconds.
2. A push to an uninstalled app disables that `push_devices` row, once, and does not disable
   anything else.
3. A healthy push produces a receipt with `status: "ok"` and no log noise — the quiet path stays
   quiet.
4. `getReceipts` being unreachable does not fail the send; it retries on the queue and gives up.

## Out of scope

- Retrying delivery. A receipt is a verdict, not a retry signal; a Device Gate that failed should be
  re-triggered by a person, not silently re-sent.
