# Launch readiness — self-serve, Stripe, paying customers

**Goal:** what stands between today and a stranger paying for Sentinel without anyone helping them.
**Scope chosen 2026-08-02:** home/prosumer **self-serve**, **Stripe** as the live payment path.

## The good news first: the money path is largely built

This audit expected to find a payments gap. There isn't one.

| Piece | State | Evidence |
|---|---|---|
| Self-serve signup | **works** | `POST /auth/signup`, and the login page has a real signin/signup toggle |
| Trial on signup | **works** | signup calls `start_trial` — 30-day Beta, 2 cameras, best-effort so a trial failure can't fail the signup |
| Plans + prices | **built** | `billing/plans.rs`: per-tier camera cap, retention days, included GPU/storage/notifications; prices per currency |
| Stripe | **built and configured in production** | `billing/payments/stripe.rs`; `STRIPE_API_KEY`, `STRIPE_WEBHOOK_SECRET`, `PAYMENT_PROVIDER` all present in `/ibbf/sentinel.env` |
| Subscribe flow | **built** | `billing/+page.svelte` calls `billing.subscribe()` |
| Invoices + metering | **built** | `billing/invoices.rs`, `billing/storage.rs` |

A user can sign up, get a trial, and subscribe. **The blockers are not commercial plumbing — they are the things that keep a customer once they arrive.**

## Blocker 1 — there is no email. At all.

`grep -rniE "smtp|sendgrid|resend|postmark|mailgun|lettre|ses"` across the workspace returns **nothing**, and production has no `SMTP_*`/`MAIL_*`/`EMAIL_*` variable. The notification module delivers **Telegram and push only**.

For a self-serve product that is not a missing feature, it is a missing organ:

- **No password recovery.** `POST /users/:id/password` is an *admin* setting *another user's* password, gated on `users.password_reset`. A self-serve customer who forgets their password has **no path back into their account** — no "forgot password" exists, because there is nothing to send a link with. For a consumer product this is the single largest silent churn source.
- **No email verification**, so any typo'd address is an unreachable, unrecoverable account that has already been charged.
- **No receipt.** Stripe can send its own, but the product sends nothing.
- **No trial-expiry warning.** The 30-day trial ends in silence — the moment a customer is most likely to convert, and they hear nothing.
- **No welcome / onboarding.** The moment of highest intent, unused.

Cards [[#70]] (branded email template system) and [[#65]] (invite + set-password email) both assume a delivery layer that does not exist. **A provider has to be chosen and wired first** — that is card #165 below, and it blocks the other two.

## Blocker 2 — no Terms of Service, no Privacy Policy

Neither exists in the repo. That is a problem in three directions at once:

1. **Stripe** expects a business with published terms and a refund policy before it processes consumer payments; accounts get reviewed.
2. **The product is cameras.** Video of people's homes is sensitive personal data under both GDPR and LGPD. Selling it to consumers without a privacy policy, a stated retention period, and a deletion path is not a paperwork gap.
3. **Nothing records consent** at signup.

The retention machinery already exists (`event_retention_days`, the nightly sweep, clip TTLs), so the policy can describe something true rather than aspirational — the honest version is cheap to write here, which is unusual and worth using.

## Blocker 3 — CI compute minutes are exhausted (found while auditing)

Every job on !23 failed with `ci_quota_exceeded`. The namespace is `ultimate_trial` with a 400-minute limit, and it is spent.

**Nothing can merge or deploy until this is resolved** — `main` is protected and requires `check:rust` to pass. Today's releases used a lot of that budget. This is a launch blocker in the most literal sense: you cannot ship a fix to a paying customer right now.

Note this also corrects an earlier claim in this session that the group was on Free with no macOS-runner option: the namespace reports `ultimate_trial`, so GitLab's hosted macOS runners (Premium/Ultimate) may in fact be available while the trial lasts — worth checking against card #163 before the trial ends.

## What I recommend building, in order

| # | Card | Why this order |
|---|---|---|
| 1 | **#165 Email delivery foundation** | Unblocks #166, #65 and #70. Nothing else in the funnel can be fixed without it |
| 2 | **#166 Self-service password reset** | The one gap that silently loses customers who already wanted to pay |
| 3 | **#167 Terms + Privacy + signup consent** | Gates taking consumer money at all; cheap because retention is already real |
| 4 | **#168 CI compute budget** | Buy minutes or cut pipeline cost — otherwise none of the above can ship |
| 5 | #70, #65 | The branded template system and invite journey, on top of #165 |

## The journey, once those exist

```
land → sign up ──► welcome email (what to do first)
                   verify email
      → install the edge, pair the box, add a camera
      → first motion event ──► "your first alert" email (the aha moment)
      → day 23 ──► trial ending, here's what you'd keep
      → day 30 ──► converted (receipt) or expired (what happens to my clips)
      → ongoing ──► receipts, payment-failed dunning, retention/deletion notices
```

Every arrow marked "email" is currently a no-op. The funnel is not leaky; it has no walls at those points.

## Deliberately not in scope

- **Asaas / Brazilian payments.** Stripe was chosen as the live path; Asaas stays built but unexercised.
- **Pricing strategy.** This audits whether you *can* charge, not what to charge.
- **Marketing site copy and SEO.** Separate discipline, no engineering blocker.
