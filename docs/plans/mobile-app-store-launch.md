# Olhar AI on the App Store — what launch actually requires

**Board card:** #185 Mobile production launch — App Store review readiness
**Goal:** A build that passes review and a repeatable way to ship the next one.

The app works. Everything below is about the gap between "works on Jackes's iPhone" and "Apple
lets other people install it".

## What is already right

Worth stating, because it is most of the hard part and it is done:

- `PrivacyInfo.xcprivacy` exists in the app target (required since 2024 — a missing one is an
  automatic upload rejection).
- `aps-environment` entitlement is present, so production push is wired.
- Bundle id `com.olharai.app`, Apple Team `3A4H2Y6ZVM`, App Group `group.com.olharai.app`, and an
  EAS project id already exist in `app.json`.
- `eas.json` has a `production` profile with `autoIncrement` and `appVersionSource: remote`, so
  build numbers are handled.
- The widget target ships via `@bacons/apple-targets`.

## Blockers, in the order they will bite

### 1. In-app account deletion — guideline 5.1.1(v)

**This is a rejection, not a warning.** Any app that lets someone *create* an account must let them
delete it from inside the app. Since the invite flow (#181) creates accounts on the phone, this now
applies to us squarely.

There is no delete path in `sentinel-mobile` at all — `session.ts` has `signOut`, which is not the
same thing and Apple says so explicitly.

Needs: a control-plane endpoint for self-deletion, and a Settings entry that explains what is
destroyed. **Open question below**, because "delete my account" is genuinely ambiguous for a tenant
owner.

### 2. The legal documents still say `[LEGAL ENTITY NAME]`

App Store Connect requires a privacy policy URL, and a reviewer opens it. Ours renders
`sentinel-app/src/lib/legal/documents.ts`, whose `OPERATOR` constant is four unfilled placeholders —
entity, address, contact, privacy contact — and `docs/legal/README.md` states plainly that the text
has not been reviewed by counsel.

This is card **#167**, already In Progress, and it is now a launch blocker rather than a nice-to-have.
Filling the placeholders is ours; the review is not.

### 3. Face recognition will be reviewed closely

This app captures and matches **biometric data**. That draws scrutiny that a camera app does not:

- The App Privacy answers must declare biometric collection honestly, and they must match what the
  Privacy Policy says. A mismatch between the two is a common rejection.
- The camera permission string should say what happens to a face, not just that the camera is used.
- Expect a reviewer to ask for a demo account and a written explanation.

Getting this wrong is worse than a rejection — it is the claim most likely to be wrong in public.

### 4. Reviewer-visible strings that say "development"

`NSLocalNetworkUsageDescription` currently reads *"connects to the development server and to cameras
on your local network"*. A permission dialog quoting a development server reads as a dev build. It
also under-describes the real reason, which is LAN cameras.

### 5. Version is `0.1.0`

`app.json` and `package.json` both. A store release should be `1.0.0`; the build number is EAS's
problem, the marketing version is ours.

### 6. There is no reproducible build path

`release:mobile-ios` exists in `.gitlab-ci.yml` and calls EAS, but **`EXPO_TOKEN` is not among the
project's CI variables** (only `VPS_HOST`, `VPS_SSH_KEY`, `VPS_USER`). So the job has never run, and
every build so far has come off a laptop — the exact thing [[no-local-builds]] exists to prevent.

Creating the token and adding it masked is a five-minute task that turns an undocumented ritual into
a pipeline.

### 7. Store listing material

Not code, but it blocks submission: screenshots for 6.9" and 6.5" iPhone **and iPad** (because
`supportsTablet: true` — dropping iPad support is a legitimate way to halve this), description,
keywords, support URL, age rating, and the export-compliance answer
(`ITSAppUsesNonExemptEncryption` — we use HTTPS only, so the exemption applies, but it must be
declared or every upload stalls).

## Order of work

1. Account deletion (endpoint + Settings screen) — the only true code blocker.
2. Fill `OPERATOR`, fix the permission strings, bump to `1.0.0`.
3. `EXPO_TOKEN` → first EAS production build → TestFlight.
4. App Privacy answers, written to match the Privacy Policy.
5. Screenshots and listing.
6. Submit.

TestFlight before submission is not optional here: it is the only way to test the production
signing path, and push behaves differently on a production `aps-environment`.

## Open decisions

1. **What does "delete my account" mean for a tenant OWNER?** Deleting the only admin would strand
   cameras, boxes and recordings that other members rely on. Proposed: a member deletes themselves
   outright; an owner is told they must transfer ownership or delete the whole tenant, with the
   tenant-deletion path being explicit and confirmed. Apple accepts "you must resolve X first" as
   long as it is actionable **in the app**.
2. **Keep iPad support?** `supportsTablet: true` doubles the screenshot work and adds a review
   surface for layouts nobody has tested. Proposed: **drop it for 1.0**, add it back deliberately.
3. **Does the widget need its own review consideration?** It runs App Intents and holds no
   credentials, so no — but its screenshots help the listing.
4. **Brazil-first or worldwide?** The legal text assumes LGPD and GDPR. Proposed: launch in Brazil
   only until counsel has reviewed, which also narrows the age-rating and privacy questions.

## Out of scope

- Android / Play Store. Same app, a different checklist, and a separate card.
- Anything about the cloud services. This is the phone only.
