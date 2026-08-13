# Tuya — a journey that survives a real customer

**Board card:** #182 Tuya onboarding — diagnose the failure, prove the device moves
**Goal:** A tenant connects their Tuya project and **sees one of their devices respond**, without
guessing which of five silent failures they hit.

## What is settled, and must not be re-litigated

BYO-project is the only ToS-clean path. OAuth is OEM-only, user/password login returns
`1106 permission deny`, the QR is console-generated with no public API, and scraping the console
risks the shared operator account. The **OEM licence costs USD 5 000 the first year and USD 2 000/yr
to renew**, and the free tier is non-commercial and capped at 100 users. All of this is researched
and recorded in [[tuya]] — the decision is closed.

So this card is not "make onboarding self-serve". It is: **BYO is the journey, make it a good one.**

## The real problem

The walkthrough already exists — five steps plus a data-centre picker
(`sentinel-app/src/routes/settings/integrations/+page.svelte:255`, copy in
`src/lib/i18n/messages/integrations.ts`), and `validate` fetches a token before saving. What is
missing is everything that happens when it does **not** work, and every failure currently surfaces
as the same raw string: `tuya api error {code}: {msg}` (`crates/app-core/src/tuya/client.rs:109`).

Five distinct failures, indistinguishable today:

| What happened | What Tuya says | What the tenant must do |
|---|---|---|
| Project is in another data centre | token fails, or succeeds with **zero devices** | switch the DC |
| APIs not subscribed | `1106` / `28841105` permission deny | Service API → Go to Authorize |
| Smart Life account never linked | token fine, **zero devices** | Devices → Link App Account → QR |
| Access Secret mistyped | `1004` sign invalid | re-copy from Overview |
| Linked, but the account has no devices | zero devices | add a device in Smart Life first |

Three of those five look identical from the UI: an empty list. **The wrong data centre is the most
common**, and it reads as "your credentials are broken", which sends people to re-copy a secret that
was never wrong.

## Phase 1 — Name the failure

- A `TuyaError` enum in `crates/app-core/src/tuya/client.rs` replacing the stringly-typed `Err`:
  `Signature`, `NotSubscribed(code)`, `PermissionDenied(code)`, `Network`, `Api { code, msg }`.
  Map the codes we know (`1004`, `1106`, `2406`, `28841105`) and keep `Api` as the honest fallback —
  inventing a friendly message for a code we have not seen is worse than showing it.
- `PUT /tuya/config` and `POST /tuya/sync` (`sentinel-control/src/domain/tuya.rs`) return the
  variant, and the Integrations page renders **one sentence naming the exact console screen**.

## Phase 2 — Find the data centre for them

On a validation failure, retry the token against the other three endpoints
(`DATA_CENTERS` already lists US/EU/CN/IN). If one succeeds:

> *Your project is in **Western America**, not Europe. Switch?* — one click, then continue.

This turns the single most common failure into a fixed one. It costs at most three extra token
calls, only on a path that has already failed, and it is the difference between a support ticket and
a working connection.

## Phase 3 — A connection checklist, not a boolean

After saving, show four states rather than "connected":

1. **Credentials** — token obtained.
2. **APIs** — the calls we need are subscribed.
3. **Account linked** — `linked_uids` is non-empty (`tuya::repo`), else *"scan the QR in step 4"*.
4. **Devices** — N found, else *"the linked account has no devices yet"*.

Each failing line carries its fix. `/tuya/provider` already returns `{configured, source}`; extend
it to carry this, so the page states the truth instead of inferring it.

## Phase 4 — Prove the device moves

The moment that makes someone believe it: a **Test** action per device on the Integrations page.

- **Read status** by default (`client::get_status`) — shows the live value, changes nothing.
- **Toggle** behind an explicit confirm, because the device may be a lock or a heater and a "just
  checking" click must not open a door. Same reasoning as the Device Gate's slide (#176).
- Report the round-trip time; a cloud call to Tuya is 300–800 ms and seeing that number sets the
  right expectation before anyone builds a workflow. It is also the argument for #133 (local
  control), measured rather than asserted.

## Phase 5 — First-run finish

- Sync reports **how many devices arrived** and names the ones whose functions we could not read
  (`fetch_devices` already tolerates a failed `/functions` call — say which).
- Land straight on the labelling editor (`function_labels_json`, shipped in #131): `switch_1` means
  nothing, "Luz da Escada" is what a workflow author needs.

## Open decisions

1. **Toggle-on-test: ship it or read-only?** Proposed: ship it, behind a confirm naming the device.
   A read proves the credentials; only a toggle proves the *control* path, which is the half people
   doubt.
2. **Auto-switch the data centre, or ask?** Proposed: **ask.** Silently rewriting a saved endpoint
   is the kind of helpfulness that is impossible to debug later.
3. **Do we probe all four DCs on every validation failure, or only when the error looks regional?**
   Proposed: all four. The failure is already terminal; three extra calls cost nothing anyone can
   perceive.
4. **Does any of this need to exist on the phone?** Proposed: **no.** Connecting an integration is
   an admin-at-a-desk job; the phone consumes the result. Keeps this card small.

## Out of scope

- OEM app / OAuth / console scraping. Closed, see above.
- Local LAN control — that is #133, and it does not change this onboarding (`local_key` still comes
  from the Cloud API).
- Non-Tuya providers.
