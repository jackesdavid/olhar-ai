# Pairing progress — an intermediate "code accepted" state

**Board card:** #85 Pairing progress UX (ToDo)
**Goal:** After a customer types the pairing code into the appliance, the Boxes install view should
tell them **immediately** that the code worked and the appliance is starting — instead of sitting
mutely on the code until the box happens to check in (or forever, if it doesn't).

Refines [[box-pairing-code]] (#81). Surfaced while testing the macOS `.pkg` (#82): the code was
redeemed successfully, the box token minted — but the Settings page stayed on the code screen with
no acknowledgment, because it only flips on `last_seen`, which comes seconds-to-never later.

---

## The gap, precisely

`sentinel-app/src/routes/settings/boxes/install/+page.svelte` polls `boxApi.list()` every 4s and
flips to the success state only when the box's **`last_seen`** is set (`checkOnline`, line 60-67).
So there are **two** transitions the user cares about, but the UI only shows the second:

1. **code redeemed** — the appliance called `POST /enroll`, got its token, and is restarting. This
   happens the instant the user hits enter. **The UI shows nothing.**
2. **box online** — the restarted edge pulled cameras and stamped `last_seen`. Seconds later (or, if
   the box is slow / offline / — as on the SentinelOne test Mac — never launches), much later.

Between the two, the user stares at the code wondering if it worked. If step 2 never comes, they get
no signal that step 1 *did*.

## What control already knows

The pairing row (`box_pairings`, migration 0055) has **`claimed_at`** — set exactly when the code is
redeemed (`store::claim_pairing`). That timestamp *is* "the code was accepted." The install view just
can't see it yet.

## Phase 1 — control: expose pairing status

`GET /boxes/:id/pairing-status` (auth, `devices.read`) → the two signals the view needs:

```json
{ "claimed": true, "online": true }
```

- `claimed` — this box has a `box_pairings` row with `claimed_at IS NOT NULL` (the most recent code
  was redeemed). Cheap indexed lookup by `box_id`.
- `online` — `tenant_boxes.last_seen` is set (and recent). Lets the view poll ONE endpoint instead of
  listing all boxes.

## Phase 2 — web: three states, not two

In the install view, replace the single `online` boolean with a small state machine driven by the
poll:

1. **`waiting`** — code shown, not yet claimed. "Enter this code on the appliance." (today's state)
2. **`accepted`** — `claimed && !online`. **New.** "Code accepted — the appliance is starting up…"
   with a spinner. This is the acknowledgment that was missing.
3. **`online`** — `online`. "'<name>' is online." (today's success state)

- Keep the 4s poll; just hit `pairing-status` and map to the state.
- A named helper returns the state from `{claimed, online}` — no nested ternaries in markup.
- **Handle the stall**: if `accepted` persists beyond ~60s (box booting but not checking in — the
  launchd/SentinelOne case), add a gentle hint: "Taking longer than expected — check the appliance is
  running, or see its logs." Don't spin forever with no explanation.

## Phase 3 — the code's own expiry

While here: if the code expires (15 min) in the `waiting` state without being claimed, say so and
offer "Generate a new code" (the mint button already exists). Right now an expired code just silently
stops working.

## Verification

- Redeem a code → the view flips to **"Code accepted — starting up…"** within one poll (~4s), before
  the box is online.
- Box then checks in → flips to online.
- Redeem, then kill the edge before it checks in → the view sits on "accepted" and, after ~60s, shows
  the stall hint (not a forever spinner).
- Let a code expire unredeemed → "expired, generate a new one."

## Open decisions (grill before building)

1. **Poll `pairing-status`, or push over SSE?** The app already does SSE for workflow runs. A poll is
   simpler and the window is short (minutes); leaning poll, reusing the existing 4s loop.
2. **What counts as "online" — any `last_seen`, or recent?** A box enrolled days ago then reinstalled
   has an old `last_seen`. For the install flow, "online" should mean *checked in since this install
   started* — compare `last_seen` against the time the code was minted, not just non-null.
3. **Should `accepted` also show which box** in a multi-box tenant? The view is already scoped to the
   one box being installed (`pendingInstall.id`), so no.

## Out of scope (v1)

- Re-pairing an already-enrolled box (that's [[box-rekey-enrollment]], #4).
- Showing live boot logs in the browser during `accepted` (nice, but the edge log viewer already
  exists on the Boxes page for after it's online).
