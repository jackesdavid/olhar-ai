# A Device Gate can only ask a phone — the picker should say so

**Board card:** #186 Device Gate offers recipients that can never answer
**Goal:** The recipient picker on a Device Gate lists only what can actually approve, and the
run-time error names the reason when something else slips through.

## The answer to the question

**No — a Device Gate cannot be sent to Telegram**, and not because nobody implemented it. Three
things in the node's design make it impossible rather than missing:

1. **There is no approval screen.** The answer arrives from a screen that renders the prompt, a
   countdown and a slide — the app. A chat message has nowhere to put that.
2. **There is no factor to record.** Decision 02 of the map requires that an approval confirmed by
   passcode rather than biometrics still counts, *but that its strength is legible*. Telegram offers
   neither, so `auth_factor` would have to be blank or invented.
3. **The token must never reach anything that renders.** It is stored as `sha256` and sent in a
   push `data` field precisely so it cannot appear on a lock screen. In a chat it would be plain
   text in someone's message history.

So this is a real constraint, and the fix is to stop offering the impossible.

## What happens today

`resolve_device_by_label` (`crates/app-core/src/workflows/device_gate.rs:113`) looks the label up in
**`push_devices`** only. A Telegram beacon lives in `notification_targets`, so it finds nothing and
the node takes `failure` with:

> `Device Gate: '<name>' has no registered phone`

That is correct, and late: it happens on the first real run, after the graph is armed, to whoever
was relying on the approval.

The editor makes it easy to hit. `+page.svelte:1825` merges both sources into one flat list:

```js
recipients = [...beacons, ...phoneRows];   // label + enabled, no kind
```

Every Device Gate dropdown therefore offers every Telegram target, indistinguishable from a phone.

**A second bug in the same merge:** a phone whose label collides with a beacon is *dropped*
(`!r.some((b) => b.label === p.label)`). So naming a phone after an existing Telegram target makes
the phone vanish from the picker with no message — and that phone is the only thing that could have
answered a gate.

## Phase 1 — carry the kind, filter the picker

- `recipients` becomes `{ label, enabled, kind: "telegram" | "phone" }`. Both sources already know
  which they are; the information is discarded at the merge, not unavailable.
- The `recipient` dropdown filters by node kind: `device_gate` lists **phones only**; everything
  else keeps the full list.
- When a tenant has no phone at all, the Device Gate's picker says so and points at the app —
  an empty dropdown is indistinguishable from a broken one.

## Phase 2 — stop dropping colliding phones

Show both, disambiguated (`Kitchen (phone)` / `Kitchen (Telegram)`) rather than silently preferring
the beacon. The label is the addressing key on both sides, so a collision is a genuine ambiguity —
but it is one the author should resolve by renaming, having been told, not one we resolve by
hiding a device.

## Phase 3 — say why, at run time

A graph built before this, or one built through the API, can still name a beacon. `resolve_device_by_label`
should distinguish the two cases:

- no such label anywhere → *"no recipient named X"*
- the label is a **Telegram** target → *"X is a Telegram recipient; a Device Gate needs a phone
  running the app, because the approval is confirmed with Face ID or a passcode"*

One extra query on a path that is already failing, and it turns a dead end into an instruction.

## Verification criteria

1. A Device Gate's recipient dropdown, in a tenant with both a Telegram beacon and a registered
   phone, lists **only the phone**.
2. A `Notify Recipient` dropdown in the same graph still lists **both**.
3. A phone and a beacon sharing a label both appear, each labelled with its kind.
4. A graph whose Device Gate names a beacon (set through the API) fails with the Telegram-specific
   message, not the generic one.
5. A tenant with no registered phone sees an explanation in the picker, not an empty list.

## Open decisions

1. **Filter in the editor, or make it a type?** A distinct `PortType` (say `Phone`) would let the
   canvas refuse the wire outright, which is stronger than a filtered dropdown — the editor already
   enforces port types. But it ripples: `Find Recipient` would need to emit it, or gain a kind
   selector that types its output the way `variable_port` does. Proposed: **filter now, type
   later** — the filter is a day and removes the whole class from the UI; the type is the right
   long-term shape and belongs with #102's parameterisation work.
2. **Should `Notify Recipient` warn when a phone is named but push is not registered?** Out of
   scope here, but the same shape of problem.

## Out of scope

- Any change to how a Device Gate is answered. Push + the app is the mechanism, by design.
- Email or SMS approval. Same three objections as Telegram, and worse.
