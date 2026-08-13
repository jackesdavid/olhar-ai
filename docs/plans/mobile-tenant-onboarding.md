# Mobile onboarding — get a phone into a tenant

**Board card:** #181 Mobile onboarding — invite a person, pair their phone
**Goal:** An admin invites someone; that person installs the app, scans a QR (or types a short
code), and is inside the tenant with push working and their phone named — without anyone reading a
password aloud.

## Current state — why this blocks launch

The only way into the app today is `signIn(email, password)` against `/auth/login`
(`sentinel-mobile/src/session.ts:48`). For that to work the person must already be a user, and the
only way to become one is `invite_user` (`sentinel-control/src/handlers.rs:251`), which takes an
**admin-chosen password**, sends **no email**, and tells the invitee nothing. So the real-world
journey is: an admin types a password into a form and then tells someone what it is. That is not
shippable to customers.

Three things already work and must be reused, not rebuilt:

- **`expo-camera` is already a dependency** (`sentinel-mobile/package.json`) — QR scanning needs no
  new package.
- **Push registers itself at sign-in** (`login.tsx` → `registerThisDevice()`), and a phone can be
  renamed from the product (#139). Onboarding just has to feed it a name.
- **Card #65** ([[invite-onboarding-email]]) already plans the transactional-email primitive and the
  set-password link. This card is the **phone** half; the two share the invite token.

One landmine worth naming: the mobile defaults point at `control.thesentinels.online`, because
`control.olharai.com` does not resolve (`sentinel-mobile/src/config.ts`). A QR that carries the base
URL removes that whole class of "the app reaches nothing" problem — the invite decides which cloud
the phone talks to, instead of a bundle-time constant.

## The journey

1. Admin: Settings → Users → **Invite**. Picks roles. Gets a **QR + a short code** on screen (and,
   once #65 lands, an email with the same link).
2. Person: installs the app → **Scan invite** on the login screen → camera → done.
3. The app learns the **cloud URL and the invite token** from the QR, accepts the invite, gets a
   session, and asks two things: a password (so they can use the web too) and **what to call this
   phone**.
4. The phone appears in Recipients under that name, push already registered.

Typing the code is the fallback for a phone whose camera is refused — same endpoint, same token.

## Phase 1 — Invite tokens in control

- Migration: `user_invites` — `token_sha256`, `tenant_id`, `email` (nullable, so a QR invite needs
  no address), `role_ids`, `expires_at`, `accepted_at`, `created_by`. RFC3339 `text` timestamps
  (project convention), and store the **hash**, never the token — same rule the Device Gate token
  follows (`docs/ref/workflows.md`).
- `POST /auth/invites` (authed, `users.manage`) → `{ token, code, expires_at }`.
- `GET /auth/invites/:token` (**public**) → `{ tenant_name, email? }` so the phone can show *which*
  organisation it is joining before anyone commits. Never returns roles or ids.
- `POST /auth/invites/:token/accept` (**public**) → `{ password, device_name? }` → creates the user,
  grants the roles, marks the invite accepted, and mints the same `Session` shape `/auth/login`
  returns, so the app's existing session handling is untouched.
- **Single-use, and settle it the way the gate does**: one conditional
  `UPDATE … WHERE accepted_at IS NULL RETURNING`, the row count decides. A check-then-act here means
  two people join from one invite.
- Rate-limit `accept` and `GET` by token — these are unauthenticated and enumerable.

## Phase 2 — The QR (web)

- `sentinel-app/src/routes/settings/users/+page.svelte`: after creating an invite, show the QR and
  the code with a copy button and the expiry.
- **QR payload is JSON**, not a bare token: `{"v":1,"url":"https://control…","t":"<token>"}`. The URL
  is what lets one app binary serve any deployment, and it is how we stop shipping a cloud address
  as a bundle constant.
- Keep the current admin-sets-password path working until #65 ships the email; this is additive.

## Phase 3 — The phone

- `sentinel-mobile/app/invite.tsx`: camera scan (expo-camera) + a "type the code instead" field.
- On a valid payload: persist the base URL (SecureStore, alongside the token), call
  `GET /auth/invites/:token`, and show **"Join <tenant>?"** before asking for anything.
- Accept → password + phone name → session stored → `registerThisDevice()` with that name →
  `/(tabs)`.
- `CONTROL_BASE` becomes "the stored URL, falling back to the compiled default", touched in one
  place (`src/config.ts`) so nothing else has to know.
- Login screen gets a **Scan invite** button; a person with no account should not have to find it.

## Phase 4 — Prove it

Simulators cannot scan, so this needs a real device (see [[mobile-test-on-device]]):

1. Invite → scan → join → the tenant name shown is right, and push arrives.
2. The same QR a second time → refused, and says it was already used.
3. An expired invite → refused, and says so.
4. Typed code path → same result as the scan.
5. A QR pointing at a different cloud URL → the app talks to that one.

## Open decisions

1. **Does accepting require setting a password?** Proposed: **yes, but after the session is
   minted** — the phone works immediately, and the password exists for the web. Making it a
   precondition puts a keyboard between someone and a working app.
2. **Invite without an email address?** Proposed: **allow it.** The QR is handed over in person,
   which is the common case for a site technician, and requiring an address invents a fake one.
3. **Expiry.** Proposed 7 days, configurable later. Long enough for a real install, short enough
   that a photographed QR is not a permanent door.
4. **Does the invite carry roles, or a single "Viewer" default?** Proposed: roles, chosen by the
   admin at creation — anything else means every invited person needs a second admin action.

## Out of scope

- The invite **email** — that is #65, and this card is deliberately usable without it.
- Self-serve tenant signup from the phone. Joining an existing tenant is the launch blocker.
- Any change to how the JWT itself works.
