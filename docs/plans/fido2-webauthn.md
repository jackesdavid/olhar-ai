# FIDO2 / WebAuthn passkey login — Implementation Plan

**Board card:** #147 FIDO2 / WebAuthn passkey login
**Goal:** Hardware-key / passkey (WebAuthn) authentication on login — the root fix for a
phished/defaulted password — rolled out per-user so it never locks the fleet out at once.

## Context

This is **§2 of the security-hardening plan** (`~/.claude/plans/do-security-research-in-snazzy-wreath.md`).
The rest of that plan shipped — fail-closed secrets, webhook replay window, login lockout
(`crates/app-core/migrations/0078_login_lockout.sql`), and the per-box HMAC signing (§3:
`crates/app-core/src/signing.rs`, `sentinel-control/src/box_signing.rs`, `sentinel-edge/src/signer.rs`,
migration `0077_box_signing_keys.sql`). **WebAuthn is the one unimplemented piece** — no `webauthn-rs`
dep, no `user_credentials` table, no endpoints. It's the root fix for §1.1 (defaulted password) and
§1.7 (login stuffing): a phished or defaulted password stops being sufficient.

"FIDO" means **WebAuthn broadly** — platform authenticators (Touch ID, Face ID, Windows Hello,
Android passkeys) as well as roaming keys. Requiring every camera-SaaS user to buy a YubiKey is
unworkable; the same API + table cover both, so it's a policy choice in the register/authenticate
options (`authenticatorAttachment` unset, `residentKey: preferred`), not extra work.

Enforcement is **required for every user (master included)** but ships as a **3-stage per-user
rollout** — a single global flag would lock everyone out at once. The app's new i18n foundation
(`sentinel-app/src/lib/i18n/`, en/pt) now exists, so all new strings flow through it.

## Phase 1 — E0: enrollment + optional passkey login (this card's core; ship early)

- **Dep:** add `webauthn-rs` to `sentinel-control/Cargo.toml` (verify current version + MSRV).
- **Migration `0079_user_credentials.sql`:** table `user_credentials` (`id`, `user_id`,
  `credential_id` bytes, `public_key`, `sign_count`, `transports`, `nickname`, `created_at`,
  `last_used_at` as RFC3339 text per house rule); add `webauthn_required bool` + `grace_until text`
  to `users` (for E1/E2 states, unused in E0).
- **Control:** new `sentinel-control/src/webauthn.rs` with four routes wired in `http.rs` (next to
  the `/auth/oauth/*` routes in `handlers.rs`):
  - `/auth/webauthn/register/{start,finish}` — authenticated (`UserClaims`).
  - `/auth/webauthn/login/{start,finish}` — unauthenticated.
  - Registration/auth state is short-lived + server-held — reuse the in-memory OAuth-CSRF pattern
    (`oauth.rs` state map), noting it is **per-pod** (control is single-replica today).
  - **RP ID = the app origin** (derive from `OAUTH_REDIRECT_BASE`/app host); wrong RP ID ⇒ keys
    silently fail to assert.
  - Session issuance mirrors `oauth.rs::google_callback` → mint the same JWT; add `amr: ["webauthn"]`
    to `Claims` (`auth.rs`) so a handler can later demand key-backed auth.
- **Frontend:** `sentinel-app/src/routes/login/+page.svelte` + `sentinel-app/src/lib/auth.svelte.ts`
  — `navigator.credentials.create/get`, base64url encode/decode of challenge/response; a "Security
  keys" section in `settings` (register/name/remove a credential). **All strings via the i18n
  modules** — new `messages/webauthn.ts` (en/pt), wired in `lib/i18n/index.ts`.

## Phase 2 — E1: enroll-after-login nudge (separate follow-on)

- After a password login, a user with **no** credential is redirected to enroll; a per-user
  `grace_until` (~14 days) lets them defer. Existing sessions unaffected.
- Push users to register a **second** credential here (recovery depends on it).

## Phase 3 — E2: enforcement (gated — do not schedule until the gates below are met)

- `webauthn_required` enforced — password alone rejected. Flip **per-user in batches**, never a
  global switch; leave individual stragglers in E1 rather than lock out a paying tenant.

## Open decisions
1. **Google OAuth interaction** — `oauth.rs` mints a session without a password. Recommend **exempt
   OAuth logins** from E2 (Google enforces its own 2FA) and state it explicitly.
2. **Recovery (the dominant cost at "everyone" scale)** — required before E2: (a) second credential
   pushed in E1, (b) operator-mediated reset in the master console, itself gated on the operator's
   own key + an audit event. A locked-out master is an outage.
3. **Mobile (Olhar AI) must land before E2** — native WebAuthn on iOS/Android is a separate
   integration (ASWebAuthenticationSession / Credential Manager + `apple-app-site-association` for
   the RP ID). Largest hidden cost of "required for everyone"; scope before committing an E2 date.

## Out of scope (this card)
- E1 redirect + E2 enforcement (Phases 2–3 above ship as their own follow-on cards once recovery +
  mobile land). This card delivers **E0 only**: enrollment + optional passkey login.
- The two small leftover §3 items (a `send_json` CI guard on the edge; the P3 flip of `signing_mode`
  to `required`) — unrelated to WebAuthn, tracked separately.
