# One name, one place

**Board card:** #198 Devices, Recipients and Integrations are split by the wrong rule
**Goal:** Every screen sits where its use frequency says it should, and every concept has exactly
one user-facing word.

## The rule

Three kinds of thing, one test — **do you come back to it?**

| Kind | Examples | Where |
|---|---|---|
| **Devices** — what the system watches and controls | cameras, Tuya devices | user menu (daily) |
| **Recipients** — who gets told | Telegram contacts, phones | user menu (weekly) |
| **Integrations** — connections you establish once | enroll an edge box, link a Tuya account, pair the Telegram bot | Settings (rarely) |

A camera you look at constantly; a Tuya account link you set up once. Today both live under
Settings, which is why nothing is findable.

## What is misplaced

1. **The Tuya device list and its per-control console are inside Integrations** (#191, shipped
   2026-08-18). Wrong side of the line, and this is a correction to that card, not a discovery
   about old code: the *account link* and the diagnosis checklist are integration; the *devices*
   belong beside cameras.
2. **`/settings/devices` is the most-used screen in the least-visited menu.**
3. **Edge boxes are one entry doing two jobs.** Enrolling a box is an integration; a box's health
   is something you check when something breaks. Only the second is a "device" in any useful sense —
   nobody *uses* an edge box.

## The naming problem, concretely

The route is `/beacons`; every label says "Recipients". `beacon` is internal vocabulary the brand
guide says not to leak, so the same concept reads differently in the URL bar and on screen.

The phone side is worse — **four names for one thing**: `push_devices` (table), "Phones" (table
header), "Apps" (#189's proposed tab), "Phone Recipient" (conversation). Pick one and use it in the
route, the nav, the table header, the workflow dropdown and the error text.

**Proposed: "Phone".** It is what a person calls the object. It handles the iPad case badly but
honestly — a list called Phones containing an iPad is a smaller lie than one called Apps containing
a phone.

## Target structure

```
Devices                      ← user menu
  Cameras
  Smart home                 (Tuya devices + per-control console)
  Edge boxes                 (health / status only)

Recipients                   ← user menu
  Telegram
  Phones

Settings › Integrations
  Edge boxes                 (enroll a new one)
  Tuya                       (link the account, diagnose)
  Telegram                   (bot + pairing code)
```

## Phases

**Phase 1 — one word each.** Settle `Recipients` / `Telegram` / `Phones` / `Cameras` / `Smart home`
and apply them everywhere including the routes (`/beacons` → `/recipients`, redirect kept). Cheapest
phase, and it is what makes the rest legible.

**Phase 2 — move Devices out of Settings** and give it the three sections. No behaviour changes.

**Phase 3 — split the Tuya screen.** Device list + console move to Devices; connect flow and
diagnosis stay in Integrations. Same for edge boxes: enroll in Integrations, health in Devices.

## Supersedes part of #189

#189 proposed Recipients tabs *Onboarding · Telegram · Apps*. Under this rule there is **no
Onboarding tab** — enrollment lives in Integrations, which is already what shipped for Telegram in
#191. #189 shrinks to two tabs, **Telegram and Phones**, or folds into this card entirely.

## Verification criteria

1. One user-facing word per concept, identical in nav, headers, dropdowns and errors.
2. Nothing you use daily requires opening Settings.
3. A Tuya device is managed in the same place as a camera.
4. `/beacons` still resolves.

## Out of scope
- Any change to what the screens *do*. This is placement and vocabulary only.
