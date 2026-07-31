# Box health copy — say what's wrong, not how we built it

**Board card:** #93 Box health copy leaks internals ("tailnet", "Tailscale") to users (ToDo)
**Goal:** Box health messages should describe the user's problem in the user's terms. Today they name
our implementation, which tells the user nothing they can act on — and tells them more about our
infrastructure than they should have to know.

---

## Why

The Boxes page currently renders:

> **MacInstalledBox: offline on the tailnet — Live can't reach it (check Tailscale on the box).**

Three things wrong with that one line:

1. **"tailnet" and "Tailscale" are our plumbing, not the product.** Sentinel is "a calm assistant for
   home + business" in plain terms (Recipients / site / zones — see [[product-branding]]). A customer
   never agreed to learn what a tailnet is. The appliance is supposed to make private networking
   invisible; naming it in an error undoes that.
2. **"check Tailscale on the box" is not an action a customer can take.** They didn't install
   Tailscale; our installer did. There's no Tailscale UI we've told them about, and on the appliance
   targets there's no desktop at all to check it on.
3. **It's leaking architecture.** It tells anyone reading the screen that boxes are reachable over a
   private mesh, which is a detail about how the cloud reaches the LAN.

`tailscale_state()` (`sentinel-edge/src/tailscale.rs`) reports `disabled | offline | up | serving`.
Those are the right *internal* states — the mistake is rendering them, and their vocabulary,
straight through to the user.

## The shape

- **User-facing copy** describes symptom + action, never the mechanism:
  - `offline` → "This box can't be reached right now. Live video and snapshots won't work until it
    reconnects." + a Retry/Restart affordance. If we know it's recent, "It last checked in 20 minutes
    ago."
  - `up` (joined, not serving) → "This box is connected but isn't sharing video yet — it should fix
    itself in a few minutes." (After #92 it genuinely does.)
  - `disabled` → not an error at all; don't alarm.
- **Operator-facing detail** (`tailscale_state`, the MagicDNS host, `last_seen`) stays available for
  us — behind a "Technical details" disclosure, in the box log viewer, or admin-only. Don't delete
  it, just stop leading with it.
  on the Live path — match that bar here.)

## Open decisions

1. **Does the user get a Restart button here?** After #92 the box self-heals, so a wait-and-see
   message may beat inviting a restart. Leaning: show the state honestly, offer Restart only once
   it's been unreachable past some threshold.
2. **How much detail behind the disclosure** — full `tailscale_state` + host, or a support code?
   Leaning: full detail, admin-only; it's what we'd ask for on a support call anyway.

## Out of scope

- Changing `tailscale_state` itself or the health protocol (#92 territory).
- The Live tab's own error copy (#89 Phase 3 already covers "the appliance isn't reachable").
