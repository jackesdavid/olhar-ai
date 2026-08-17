# A phone is a Recipient, but the screen only half-treats it as one

**Board card:** #187 Recipients: a phone can't be tested or deleted
**Goal:** The phones table on Recipients offers the same three actions the Telegram table does —
rename, **test**, **remove** — because a phone is addressed exactly like a beacon and an operator
proves delivery the same way.

## What's actually missing

Almost nothing on the backend. Both endpoints already exist and are already permission-gated on
`beacons.write`, alongside the Telegram ones:

| Action | Telegram | Phone | Web UI |
|---|---|---|---|
| rename | `PATCH /beacons/:chat_id` | `PATCH /push/devices/:id` | both ✓ |
| test | `POST /beacons/:chat_id/test` | `POST /push/devices/test` | **phone missing** |
| delete | `DELETE /beacons/:chat_id` | `DELETE /push/devices/:id` | **phone missing** |
| toggle | `POST /beacons/:chat_id/toggle` | — | see decision 2 |

`sentinel-control/src/domain/push_devices.rs:129` is `test_send`: it takes an optional `label`,
resolves `push::enabled_tokens(db, tid, label)` and sends "Test notification — your phone is set
up." `remove` (`:106`) deletes by id and 404s if it isn't this tenant's.

`sentinel-app/src/lib/data.ts:566` already exposes `listPushDevices`, `renamePushDevice` and
`deletePushDevice` — only `testPushDevice` is absent. The gap is the table itself: the phones rows
in `sentinel-app/src/routes/beacons/+page.svelte:601-610` render a single Rename button where the
Telegram rows (`:549-563`) render test + delete.

## Phase 1 — the two buttons

- Add `testPushDevice(label?: string)` → `POST /push/devices/test` to `data.ts`, next to its
  Telegram twin.
- Phones rows get the same three-button group as beacons, in the same order, with the same
  spinner-while-testing state (`testingId` is currently `number | null` for `chat_id`; phones key on
  a uuid `string`, so it needs a second piece of state rather than a widened type — a beacon and a
  phone must be able to be mid-test independently).
- Delete reuses the existing confirm dialog. It is currently typed to `NotificationTarget`; it needs
  to hold either kind and say which one it is about.

## Phase 2 — a delete that tells the truth

Deleting a phone is **not** the same as renaming one. `rename` repoints every graph that named the
old label (`repo::rename_recipient`); `delete` repoints nothing, and it cannot — there is no new
label to point at. So the dialog must say what will break before it breaks:

> *Remove "iPhone do Jackes"? 2 workflows address this phone. They will fail at that step until you
> pick another recipient.*

That count comes from the same query `rename_recipient` uses, read-only. **Failing is the correct
outcome** — the alternative is silently re-addressing someone else's phone, which is #188.

## Audit — what shipped (2026-08-17, `ef90e90`)

Phase 1 in full: Test and Remove on the phones table, `testPushDevice` in `data.ts` always passing
the label, a second `testingPhoneId` so a phone and a beacon can be mid-test at once, and one
delete dialog serving both kinds. Validated on production by the operator.

Phase 2 **partially**: the dialog names the consequence ("any workflow that addresses this phone
will fail at that step") but does **not** count the affected workflows — that needs a read-only
endpoint over the same query `rename_recipient` uses, which was not built. The sentence is honest
without the number; the number would make it actionable. Left for whoever picks up #188, since the
two are the warning and the safety net for the same hazard.

Open decision 2 (toggling a phone like a beacon) remains untouched and unclaimed.

## Verification criteria

1. Test on a phone row delivers a push to that phone, and only that phone.
2. Test on a phone with push mis-configured surfaces the server's reason (this is the surface that
   would have caught the Expo 403 in seconds instead of through a workflow run).
3. Deleting a phone removes it from the list, from the Device Gate picker, and from
   `push/devices`; a second delete of the same id 404s rather than 500s.
4. The confirm dialog names the number of workflows that address it.
5. A tenant with no phones sees the empty state, not an empty table.

## Open decisions

1. **Test one phone, or all of them?** The endpoint takes an optional label and falls back to every
   enabled device. The button is per-row, so it must always send the label — a "test" that pushes to
   the whole household when someone meant to check one device is a bad surprise. Proposed: always
   pass the label; no all-devices button.
2. **Should a phone be toggle-able like a beacon?** `push_devices.enabled` exists and
   `enabled_tokens` honours it, but nothing writes it from the UI. Out of scope here; worth a card
   if someone wants to mute a phone without unregistering it.

## Out of scope

- Registering a phone from the web. That only ever happens from the app, after login.
- Any change to how tokens are stored or refreshed.
