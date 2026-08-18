# Integrations in tabs, and a Tuya device console you can actually press

**Board card:** #191 Integrations screen in tabs + a testable Tuya device console
**Goal:** One Integrations screen with a tab per provider, and inside the Tuya tab a device console
where **every control can be read and sent individually** — so a person proves what a device does
before building a workflow on it, instead of discovering it in a graph at 2am.

Replaces #133 (LAN control), closed 2026-08-17: the answer is not to make the integration *faster*,
it is to make it *provable*.

## What exists today

`sentinel-app/src/routes/settings/integrations/+page.svelte` (541 lines) is **Tuya-only**, in one
long column: connect flow → diagnosis checklist → device list. Telegram's per-tenant config (the
pairing code) lives on a different screen entirely, `/beacons`.

Per device, expanding a row lists its controls as `code` + `type` + a friendly-name input. There is
one **Test** button, on the whole device: it reads status and changes nothing.

So today you can see that a device *answers*. You cannot see what any single control *does*.

## The finding that makes this cheap

**The data needed to render real controls is already stored, and the UI throws it away.**

`crates/app-core/src/tuya/client.rs:324-342` builds each cached function from the v2.0 thing model
with four fields:

| Field | Meaning |
|---|---|
| `code` | `gate1`, `gate1_fca`, … |
| `type` | `Boolean` / `Enum` / `Integer` / `String` |
| `values` | Tuya's own JSON **string**: an enum's `range`, an integer's min/max/scale |
| `access` | `rw` / `ro` / `wr` — **which is why a limit switch must not render as a button** |

The page's `functionsOf()` (`:60-68`) maps this down to `{code, type}` and drops `values` and
`access`. Restoring them is a one-line change; everything below hangs off it.

## Phase 1 — tabs

`Tabs` from shadcn-svelte on `/settings/integrations`: **Telegram** and **Tuya**, tab in the URL
(`?tab=tuya`) so a reload and a deep link both work. Each provider's tab shows its own connection
state, so "is this connected?" is answered by looking at one place per provider rather than by
knowing which screen a provider happens to live on.

## Phase 2 — sort controls by channel

A multi-channel module (this one has `gate1`, `gate1_fca`, `gate2`, …) currently lists controls in
whatever order Tuya returned them, so a channel's parts end up scattered.

Sort key: **the first integer in the code**, then the code itself.

```
gate1, gate1_fca, gate2, gate2_fca, …          ← numbered, grouped by channel
luz, countdown, …                              ← unnumbered, alphabetical, last
```

`gate1` sorts before `gate1_fca` naturally, since the number ties and the code breaks it. Channel
first (not name first) is deliberate: when testing channel 1, its command AND its limit switch are
what you want side by side — they are the pair that tells you whether the command worked.

## Phase 3 — a console per control

Each control renders by its `type`, and **`access` decides whether it is pressable**:

| `type` | `access: rw` / `wr` | `access: ro` |
|---|---|---|
| Boolean | toggle + Send | current value, read-only |
| Enum | select over `values.range` + Send | current value |
| Integer | number/slider bounded by `values` min/max | current value |
| String | text field + Send | current value |

Plus, per control: a **Read** that fetches just this point and shows the value with its round-trip
time, and a **last result** line (sent → what came back → how long).

**A read-only point must never render a button.** `gate1_fca` is a limit switch: it reports whether
the gate is closed. Offering to "set" it invites someone to think they closed a gate they did not
touch, and #182 already cost a real gate opening on an unverified assumption.

**Every write asks first.** These are physical devices — a gate, a lock. The confirm names the
device, the control and the value being sent.

## Phase 4 — the endpoints: NOT NEEDED

Found while building: `POST /tuya/devices/:id/test` **already sends**. Its `TestReq` is
`{ code, value }` — with both present it calls `send_command`, with neither it calls `get_status`
(`sentinel-control/src/domain/tuya.rs`), and `data.ts:293` already types the optional argument. So
the console is pure front end; no new route, no new permission surface.

The name is now a lie — "test" writes to a gate — but adding a second route that does the same
thing would be worse than a bad name. Left as is, recorded here.

## Verification criteria

1. Both tabs render; `?tab=tuya` deep-links; a reload stays put.
2. On the gate module, `gate1` and `gate1_fca` appear adjacent, and `gate2` after them.
3. `gate1_fca` (read-only) shows a value and **no** send affordance.
4. Sending `gate1` asks for confirmation naming the device and value, then reports the round trip.
5. Reading a single control after a send shows the changed value — the pair that proves the write.
6. A device that is offline says so per control rather than failing silently.

## Audit — resolved (2026-08-18, validated in production)

1. **Telegram config moved to Integrations.** Confirmed by the owner. `#189` amended: its Telegram
   tab keeps only the paired-recipient table, so the two screens do not both grow a "Telegram" tab
   meaning different things. Recipients still reads the config (not edits it) for the enrollment
   link in its empty state, which now links here.
2. **Sort is channel-first.** Reads correctly on the Garen board: `door_control_1` + `gate1*` under
   Channel 1, `gate2*` under Channel 2, `luz`/`panic`/`social_gate*` under Device. Revisit only if a
   device turns up with many unrelated numbered points.
3. **No polling.** One manual Read after a send is enough to prove the write. Continuous refresh is
   what #184 needs anyway; it stays out of here.

**What the work assumed.** That `access` is trustworthy — a point marked `ro` is genuinely
unwritable. If Tuya ever mislabels one, the console hides a control rather than offering a
dangerous one, which is the safe direction to be wrong in. Devices synced before the thing model
was read carry no `access` and are treated as writable, matching the old behaviour rather than
hiding every control on a stale device.

**Security boundary.** Gated on `tuya.write`, whose catalog description said "link a Tuya account
and edit device labels" and now also opens gates. Description corrected in `rbac.rs`; splitting a
real `tuya.control` out is card **#192**. No credential is rendered or logged by the console — it
sends `{code, value}` and shows what came back.

**A layout revision happened.** The first cut put identity, value, widget, Send and the name input
in one flex row; widget widths vary, so no two Send buttons aligned. Split into `Test | Names`
modes on fixed grid tracks. Recorded because "it built and deployed" was true of the bad version
too.

## Out of scope

- LAN/local control (#133, closed).
- Any change to the Tuya workflow nodes or their selectors.
- Adding providers beyond Telegram and Tuya; the tab structure should just make that trivial.
