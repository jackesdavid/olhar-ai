# User Invite — Onboarding Email + Confirmation Flow — Implementation Plan

**Board card:** User invite: onboarding email + set-password confirmation (ToDo)
**Goal:** When an admin invites someone, that person gets a real onboarding email with a secure
accept link, sets *their own* password, and lands confirmed — instead of the admin typing a
password and handing out credentials by hand.

## Current state (the gap)

- `invite_user` (`sentinel-control/src/handlers.rs:251`) takes `{ email, password, role_ids }`,
  hashes the admin-chosen password, calls `store::create_user` (`store.rs:59`) and grants roles.
  **No email is ever sent.** The invitee has no idea they exist.
- The frontend invite form (`sentinel-app/src/routes/settings/users/+page.svelte`) collects that
  admin-chosen password (`invitePassword`, `canInvite` requires len ≥ 8).
- **No transactional-email infrastructure exists** — no SMTP/Resend/SendGrid anywhere. The
  `sentinel-notification` service and `app_core::notification` only handle Telegram/beacons.
- Self-serve `signup` (`handlers.rs:82`) creates the tenant + admin with no email confirmation.

## Phase 1 — Transactional email primitive

- New module `crates/app-core/src/email/` (mod + client + templates). Send via an **HTTPS API
  provider** (Resend) using `reqwest` (already in the tree for Asaas billing) — **not SMTP**, to
  stay rustls-only / no OpenSSL (see project convention).
- Config from env: `EMAIL_API_KEY`, `EMAIL_FROM`, `APP_BASE_URL`. Wire through
  `app_core::config` and the control-plane `AppState`.
- Minimal typed API: `email::send(to, subject, html, text)` + small render helpers per template.
- **Dev fallback:** when `EMAIL_API_KEY` is unset, log the rendered email + link to stdout
  instead of calling the provider, so local invites work without a key.

## Phase 2 — Token-based invite (replace inline password)

- Migration **0050** `user_invites`: `id uuid`, `tenant_id`, `email`, `role_ids uuid[]`,
  `token_hash text`, `invited_by uuid`, `expires_at text` (RFC3339 via `now_rfc3339()`),
  `accepted_at text NULL`. Store only a hash of the opaque token; the raw token lives only in the
  emailed link.
- Rework `invite_user`: drop `password` from `InviteReq`. Validate email + roles, create a
  `user_invites` row with a fresh token (e.g. 7-day expiry), send the onboarding email
  (`APP_BASE_URL/invite/<token>`). Do **not** create the `users` row yet — no half-users in the
  list or in permission checks.
- `users.manage` still gates the action (`rbac.rs:22`).

## Phase 3 — Accept endpoint + set-password page

- Public routes (no auth): `GET /invites/:token` → validate (exists, unexpired, unaccepted) and
  return `{ email, tenant_name }`; `POST /invites/:token/accept` → `{ password }` → in one tx:
  `create_user` with the hashed password, `grant_role` for each `role_ids`, mark `accepted_at`.
  Return an auth token (auto-login) or 204 + redirect to `/login`.
- Frontend `/invite/[token]/+page.svelte`: fetch invite, show email (read-only) + set-password
  form, submit accept, then route into the app. Reuse the login page's dot-field styling.

## Phase 4 — Confirmation + welcome emails

- On successful accept, send a **welcome email** (you're in, here's the workspace link).
- Optional: gate self-serve `signup` behind an email-confirm link too (own `email_verifications`
  token), or defer — see open decisions.

## Phase 5 — Invite management UI

- Settings → Users: drop the password field from the invite form; invite by email + roles only.
- Show **pending invites** (email, invited-by, expiry) with **Resend** and **Revoke** actions
  (`POST /invites/:id/resend`, `DELETE /invites/:id`). Keep markup logic in named helpers (house
  style), shadcn dialogs (no `window.confirm`).

## Open decisions (grill before building)

1. **Email provider** — Resend HTTP API (recommended: rustls-friendly, no SMTP/OpenSSL) vs
   `lettre` SMTP vs adding an `email` channel to `sentinel-notification`. Which?
2. **Pending-user model** — separate `user_invites` table with the `users` row created only on
   accept (recommended, plan above) vs a `users` row in a `pending`/`unconfirmed` state.
3. **Scope of confirmation** — admin-invite onboarding only for v1, or also require email
   confirmation on self-serve `signup`?
4. **Token lifetime & resend** — expiry window (7 days?), one-time use, and resend behaviour
   (new token vs extend).

## Out of scope (v1)

- Per-tenant custom SMTP / branded sender domains.
- Magic-link passwordless login.
- Rich email template designer / localization.
- SSO / OAuth-provisioned invites.
