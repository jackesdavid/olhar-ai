# Email delivery foundation — Implementation Plan

**Board card:** #165 Email delivery foundation **Blocks:** #166, #65, #70
**Plan context:** [[launch-readiness-selfserve]]
**Goal:** make it possible to send a person an email about their own account.

## Why this is first

The launch audit found the money path largely built — signup works, it starts a 30-day trial,
plans/prices/invoices exist, Stripe is live in production. What was missing was everything that
*keeps* a customer, and all of it needed one thing that did not exist anywhere in the workspace:

`grep -rniE "smtp|sendgrid|resend|postmark|mailgun|lettre|ses"` returned **nothing**, and production
had no mail variable. Notifications were Telegram and push only.

The consequence that matters most: **the only password reset is an admin setting another user's
password** (`POST /users/:id/password`, gated on `users.password_reset`). A self-serve customer who
forgets theirs has no way back into an account they are already paying for.

## Where it lives, and why

**In the notification service, as a delivery channel** — not as a separate subsystem.

`sentinel-notification`'s consumer already dispatches on a `channel` field in the job payload
(`push` vs. Telegram, card #139). Email is a third value there, which means **no migration**: the
same durable `work_jobs` queue, the same claim path, the same retry and backoff, and the same queue
view. A provider outage becomes a retry rather than a lost password reset.

What it does *not* reuse is beacon addressing. These messages go to **one person's email address
about their own account**, and must work for a user with no tenant configuration at all — so the
payload carries `to`/`subject` instead of `target`/`chat_id`. Forcing them through beacon resolution
would have meant inventing a fake beacon per user.

## SMTP, not a provider's REST API

Resend, SES, Postmark and Mailgun all speak SMTP, so this commits us to none of them: point four
env vars at whichever account gets opened and no code changes. A REST integration would have been
marginally simpler and would need rewriting the first time the provider changed — and the provider
has not been chosen yet.

`lettre` with **rustls**, never native-tls, matching the workspace rule that there is no OpenSSL to
compile or bundle.

```
SMTP_HOST      smtp.resend.com | email-smtp.eu-west-1.amazonaws.com | …
SMTP_PORT      587 (STARTTLS, default) or 465 (implicit TLS)
SMTP_USER      provider credentials
SMTP_PASSWORD
MAIL_FROM      Sentinel <no-reply@olharai.com>
```

## Unconfigured is loud

With no `SMTP_HOST`, `send` returns `EmailError::NotConfigured` instead of quietly succeeding. A
silent no-op here is a password reset that "works" and never arrives — the same class of failure as
every other bug fixed this week. An unparseable `SMTP_PORT` is also *not configured* rather than a
silent fall back to 587, which would deliver nothing on a 465-only provider.

In the consumer, `NotConfigured` **retries** like any other failure rather than being dropped: the
fix is a deployment change, and the queue is what makes the send survive until it lands.

## Tests (6, all passing)

- unconfigured is reported, not swallowed — the property the module rests on
- config read from env, default port 587, `465 → implicit TLS`
- an unparseable port is not-configured rather than a silent default
- a bad recipient fails as `Invalid` **before dialling** — so a typo is not a 20-second timeout
- html is optional; a text part always exists (renders everywhere, and html-only mail is penalised
  by spam filters)

## Out of scope

- **Templates and branding** — #70. This carries `text` + optional `html`; making them look like
  Sentinel is a separate card that now has something to build on.
- **The actual messages** — password reset is #166, invite/onboarding is #65. This is the pipe.
- **Inbound email** — no reply handling, no parsing.
- **Deliverability setup** — SPF/DKIM/DMARC records for the sending domain are a DNS task and must
  happen before real volume, or the mail lands in spam regardless of this code.

## Not yet verified against a real provider

Every test here is offline. Nothing has been sent through Resend/SES/etc. because no account exists
yet — the first real send is the proof, and the likely first failures are credentials and the
`MAIL_FROM` domain not matching a verified sender.
