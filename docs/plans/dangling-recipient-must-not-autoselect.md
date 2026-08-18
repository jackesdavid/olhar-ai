# A deleted recipient silently becomes whoever is first in the list

**Board card:** #188 A dangling recipient/camera reference auto-selects the first option
**Goal:** A node whose configured recipient (or camera, or room) no longer exists shows **empty and
in error**, and the run **fails**. It must never resolve to a different one.

## The bug, in one line

`sentinel-app/src/routes/workflows/[[id]]/+page.svelte:1067`

```js
function resolveOpt(opts, stored) {
  return opts.some((o) => o.value === stored) ? stored : (opts[0]?.value ?? "");
}
```

When the stored value is gone, this returns **the first item in the list**. It is used for the
`recipient` dropdown (`:5060`) and for cameras, so deleting a phone named `Jackes` makes every node
that addressed it display `Ana` — and the moment anything writes that node's config back, the graph
*is* addressed to Ana. A door that used to ask one person for approval now asks a different person,
with no error anywhere.

This is the worst class of bug this product can have: it does not fail, it does the wrong thing
confidently. **Failing the workflow is strictly better than substituting a recipient.**

A second instance of the same reflex is `:2443`:

```js
if ("recipient" in cfg && recipients[0]) cfg.recipient = recipients[0].label;
```

That one is on **node creation**, where defaulting to the first item is a convenience and nothing is
being overwritten. It stays.

## Phase 1 — stop the substitution (editor)

- Split the helper in two. `resolveOpt` keeps its fallback **only** where a fallback is meaningful
  (a brand-new, never-configured node with `stored === ""`). A non-empty `stored` that matches
  nothing resolves to `""` — never to `opts[0]`.
- The Select renders that as an explicit **"— missing —"** state, in the destructive colour, with
  the dead label quoted: *Recipient "Jackes" no longer exists.* The author sees which name went
  away; an empty dropdown would not tell them that.
- The node gets the same treatment the canvas already gives invalid nodes, so it is findable without
  clicking every node in a large graph.
- **Nothing auto-saves.** The config keeps the dead label until a human picks a replacement — an
  editor that silently rewrites a graph on open is how the reference was lost in the first place.

## Phase 2 — VERIFIED, no change needed (2026-08-18)

The plan said to check this rather than assume it. Checked, and the engine was already right:

- `notification::handler::resolve_target` matches a named target case-insensitively and, on no
  match, returns `Err("No target named 'X'. Available: …")` — it fails closed AND names both the
  dead label and the alternatives. The broadcast form is reachable only when the caller passes
  `None`, i.e. no recipient was set at all.
- `run_notify_recipient` always passes `Some(label)` to `push::enabled_tokens` when the label is
  non-empty, so the "every enabled device" branch cannot be reached with a set-but-unresolvable
  label. It falls through to Telegram when the label matches no phone, and Telegram then produces
  the error above.

So the run-time half of this card was a hazard that did not exist. Recording that is the point of
having written the check down: the editor was substituting silently, the engine never was.

## Phase 2 — as specified (engine)

`crates/app-core/src/workflows/node/recipients.rs` and
`crates/app-core/src/workflows/device_gate.rs` must already fail rather than fan out to everyone —
**verify this, do not assume it.** The two shapes to check:

- `push::enabled_tokens(db, tid, label)` takes `Option<&str>` and falls back to **every enabled
  device** when it is `None`. Confirm no caller can reach the `None` branch from a node whose label
  is set-but-unresolvable. A Device Gate that broadcasts to the whole household because one phone
  was deleted is the run-time twin of this bug.
- A `Notify Recipient` naming a label that resolves to nothing should take `failure` with the label
  in the message, not succeed having notified nobody.

## Phase 3 — the same rule everywhere a name is a foreign key

Cameras behave identically (`cameraValue`), and a camera deleted from a box is at least as common as
a deleted phone. Same fix, same missing state. Audit `resolveOpt`'s other call sites before closing.

## Verification criteria

1. Delete a phone addressed by a Device Gate → reopen the graph → the node shows the missing state
   naming the old label; the dropdown is **not** set to another phone.
2. Save that graph untouched → the stored config still holds the dead label, unchanged.
3. Run it → the run fails at that node with a message naming the label. It does **not** notify
   anyone else.
4. Same three for a deleted camera.
5. A newly-dropped node with no config still defaults to the first item, as today.

## Open decisions

1. **Should the editor refuse to Save a graph containing a dangling reference?** Tempting, but it
   would trap someone mid-edit who deleted a phone and is about to re-add it. Proposed: warn at
   Save, block at Arm.
2. **Delete-time warning vs. editor-time discovery.** #187 Phase 2 counts the affected workflows
   before deleting. That is the early warning; this card is the safety net for when someone deletes
   anyway. Both are needed.

## Out of scope

- Turning recipients into real ids instead of labels. The label *is* the addressing key on both
  sides today and changing that is a migration, not a bug fix. Worth its own card.
