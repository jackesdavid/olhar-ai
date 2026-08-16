# Legal documents — status and how they work

## ⚠️ These drafts have NOT been reviewed by a lawyer

The Terms of Service and Privacy Policy published at `/terms` and `/privacy` were written against
the implementation — every factual claim about retention, deletion, biometric data and what leaves
your premises was checked against the code, not assumed. That makes them *accurate*. It does not
make them *sufficient*.

**Before taking money from a consumer, both documents need review by counsel qualified in the
jurisdictions you sell into** (Brazil/LGPD and the EU/GDPR are the two the text assumes).

### Operator details — FILLED 2026-08-16

Taken from the CNPJ record. The legal name is the person because the company is an
`Empresário Individual` (natureza jurídica 213-5); JACKTECH is the trading name, and both appear
so a customer recognises one and a regulator can act on the other.

`contact@olharai.com` serves as both the general and the privacy address. A data-subject request
under the LGPD legally *arrives* there, so one mailbox that is actually read beats two that are
aspirational. Split it if the volume ever justifies a queue.

**What is still outstanding is the TEXT, not the details.** Counsel review before taking consumer
money remains required.

### Placeholders that must be filled first

`sentinel-app/src/lib/legal/documents.ts`, in the `OPERATOR` constant:

| Placeholder | What it needs |
|---|---|
| `[LEGAL ENTITY NAME]` | The company that contracts with the customer |
| `[REGISTERED ADDRESS]` | Its registered address |
| `[CONTACT EMAIL]` | General contact |
| `[PRIVACY / DPO EMAIL]` | Where data-subject requests go — must be monitored |

These render verbatim on the public pages. Shipping with the brackets visible is worse than not
publishing.

## Where the text lives, and why

`sentinel-app/src/lib/legal/documents.ts` is the single source. It sits in the repository next to
the code it describes on purpose: the Privacy Policy makes claims that are only true because of
specific implementation details, and keeping them in the same repo means a change to one shows up as
a diff alongside the other.

Both documents are bilingual (`en` / `pt`) and follow the app's language.

## Versioning — the part that is easy to get wrong

Two constants in `crates/app-core/src/legal.rs`:

```rust
pub const TERMS_VERSION: &str = "2026-08-04";
pub const PRIVACY_VERSION: &str = "2026-08-04";
```

**Bump the relevant constant in the same commit that edits the document text.**

Every signup writes a row into `user_consents` recording which document version the user accepted.
If the text changes and the version does not, every stored acceptance points at text the user never
saw — which destroys the evidentiary value of having recorded it at all. There is a unit test
asserting the constants and the required-at-signup list agree, but nothing can detect that you
edited prose without bumping a date. That one is on the author.

## What is recorded

`user_consents` — append-only, one row per (user, document) acceptance:

- `user_id`, `tenant_id`, `document` (`terms` | `privacy`), `version`
- `accepted_at` (RFC3339), `ip`, `user_agent`

A new version produces a **new row**; nothing is updated. The history of what a given user agreed to,
and when, is the whole point.

## What is NOT built yet

- **Re-consent on a new version.** Existing users are not prompted when a document changes; only
  new signups record acceptance. That needs its own flow.
- **A self-service data export or delete button.** The machinery exists
  (`clip_store::delete_tenant_objects`), but a user-facing path to trigger it does not — today it is
  an operator action, which is not a satisfactory answer to a GDPR/LGPD request at scale.
- **Cookie / tracking consent.** The app sets no analytics cookies today; if that changes, it needs
  its own banner and record.
