# Recipients, in tabs — Onboarding · Telegram · Apps

**Board card:** #189 Organise the Recipients screen into tabs
**Goal:** One screen that answers one question at a time: *how do I add someone*, *who is on
Telegram*, *which phones are registered* — instead of a 660-line stack where the pairing
instructions sit above the table they explain.

## What the page is today

`sentinel-app/src/routes/beacons/+page.svelte` renders, top to bottom, in one column:

1. the Telegram pairing config + the code/QR to send the bot,
2. the beacons table (rename / test / remove),
3. the phones table (rename only — see #187), **hidden entirely when empty**.

Two consequences. The setup instructions are permanently in the way of the list, long after setup;
and a tenant with no phone registered has no way to learn from this screen that a phone is even a
possible recipient — the section does not render at all.

## The three tabs

- **Onboarding** — how someone *becomes* a recipient. The app invite (#181) and its QR.
  **AMENDED by #191 (2026-08-17):** the Telegram pairing code and enrollment QR moved to
  `/settings/integrations?tab=telegram`, because that is where a PROVIDER is configured. This screen
  is who receives. So Onboarding here covers the app side, and links to Integrations for Telegram
  rather than duplicating a second copy of the same controls.
- **Telegram** — the beacons table alone. **Not** the bot config: that is Integrations now (#191).
- **Apps** — the phones table alone, **always rendered**, with an empty state that points at the
  Onboarding tab rather than at nothing.

"Apps" rather than "Phones": it is the app that registers, and an iPad or a second device is the
same row.

## Phase 1 — tabs, no behaviour change

`Tabs` from shadcn-svelte, three panes, the existing markup moved into them verbatim. The tab lives
in the URL (`?tab=telegram`) so a link into a specific tab works and a reload keeps its place.
Default tab: **Telegram** when the tenant has any recipient at all, **Onboarding** when it has none
— a first-run tenant should land where the work is.

## Phase 2 — the empty states

Each tab says what it is for when it is empty, and points at Onboarding. This is the phase that
makes the phones section discoverable, which is the actual complaint behind the request.

## Phase 3 — fold the invite in

Bring the app-invite QR (#181) into Onboarding so the screen is genuinely the one place a person is
added, on either channel. Depends on #181 being validated on a second device.

## Verification criteria

1. Three tabs; each renders only its own content; nothing lost from the current page.
2. `?tab=apps` deep-links; a reload stays on the tab.
3. A tenant with zero recipients lands on Onboarding.
4. A tenant with no registered phone still sees the Apps tab, with an explanation.
5. Rename / test / remove behave exactly as before the move (#187 lands independently).

## Open decisions

1. **Rename the route?** It is `/beacons`, the screen is called Recipients, and "beacon" is internal
   vocabulary the brand guide says not to leak. A redirect is cheap. Proposed: rename to
   `/recipients` in this card, keep `/beacons` redirecting.
2. **Is "Onboarding" the right user-facing word?** It is product vocabulary, not plain terms. "Add
   someone" says the same thing in the register the rest of the app uses. Proposed: label it
   **Add someone**, keep `onboarding` as the URL value.

## Out of scope

- Any change to pairing or registration mechanics.
- The Device Gate picker filtering (#186) — different screen, different card.
