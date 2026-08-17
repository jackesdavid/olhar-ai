# Mobile push — the Recipient channel that runs parallel to Telegram

A registered phone **is** a Recipient (card #139), addressed by the same label a workflow picks from
a dropdown. Everything routes through **Expo's push service**, one HTTP endpoint fronting both APNs
and FCM, so there is no per-platform branch in our code.

This doc exists because the channel failed for a full day on 2026-08-17 in a way that looked, from
every surface we own, like success.

## The path

```
node (Device Gate / Notify Recipient)
  → push::enabled_tokens(db, tenant, Some(label))     crates/app-core/src/notification/push.rs
  → post_expo(...)  POST https://exp.host/--/api/v2/push/send
      headers: authorization: Bearer $EXPO_ACCESS_TOKEN
  → Expo → APNs (iOS) / FCM (Android) → the phone
```

| Piece | Where |
|---|---|
| Send + token lookup | `crates/app-core/src/notification/push.rs` |
| Device Gate binding (one device, by row id) | `crates/app-core/src/workflows/device_gate.rs` |
| Register / rename / delete / test API | `sentinel-control/src/domain/push_devices.rs` |
| Queue consumer (fan-out) | `sentinel-notification/src/consumer.rs` |
| Registration in the app | `sentinel-mobile/src/push.ts` |
| Recipients screen (phones table) | `sentinel-app/src/routes/beacons/+page.svelte` |

**Labels are the addressing key**, and matching is EXACT (`WHERE label = $2`). A rename repoints
every graph that used the old name (`repo::rename_recipient`); a delete cannot, and does not.

`enabled_tokens(db, tid, None)` falls back to **every enabled device in the tenant**. Any caller that
can reach the `None` branch with a set-but-unresolvable label would broadcast to the whole household
— see #188.

## TWO credentials, and both must be right

They fail with the same HTTP status from different places, which is what made this expensive.

### 1. `EXPO_ACCESS_TOKEN` — ours, on the server

An **organisation-owned** Expo project has *Enhanced Security for Push Notifications* on by default,
and refuses every unauthenticated send:

```
403 UNAUTHORIZED — Insufficient permissions to send push notifications to @olhar-ai/olhar-ai
```

Lives in `/ibbf/sentinel.env` on the VPS; `sentinel-flow` and `sentinel-notification` both read that
`env_file`, so one line covers both. **A container must be RECREATED, not restarted**, to pick it up.
Optional by design — a deployment against a personal Expo project works with nothing set.

Mint one without it ever passing through a terminal:

```
mutation($d: CreateAccessTokenInput!) {
  accessToken { createAccessToken(createAccessTokenData: $d) { token accessToken { id } } } }
```
against `https://api.expo.dev/graphql` with the `expo-session` header from `~/.expo/state.json`.
(The argument is `createAccessTokenData`, not `accessTokenData`.)

### 2. The APNs `.p8` key — Apple's, uploaded to Expo

Must be created with **Environment: Sandbox & Production**. A Sandbox-only key makes every
TestFlight/App Store push die at Apple with `BadEnvironmentKeyInToken (403)`, because a store build
registers a **production** device token. Key `58958PJYH9` was exactly this; `UN3WKYR259` replaced it.

Fixing the key needs **no new build and no TestFlight upload** — push keys are server-side.
Creating one needs Apple portal Admin/Account Holder; the ASC API key EAS holds is `APP_MANAGER`,
which cannot create keys, so EAS cannot mint the replacement itself. Uploading it can be scripted:
`applePushKey.createApplePushKey` then `iosAppCredentials.setPushKey`.

## Diagnostics that actually answer

**Is the KEY valid for an environment?** Send a deliberately invalid device token to both Apple
hosts with a JWT signed by the `.p8`. APNs validates the provider token *before* the device token,
so the two failures separate cleanly — and nothing is delivered to anyone.

- `api.push.apple.com` → `400 BadDeviceToken` = key fine · `403 BadEnvironmentKeyInToken` = key wrong
- `api.sandbox.push.apple.com` → same reading, for sandbox

**Did a push actually ARRIVE?** The ticket is not the answer; the receipt is.

| Stage | Says | Do we read it? |
|---|---|---|
| Ticket (in the POST response) | Expo queued it | yes — `post_expo` |
| Receipt (`POST /--/api/v2/push/getReceipts`) | APNs/FCM verdict, with the reason | **no** — #190 |

**What does the entitlement in a shipped build say?** Download the IPA from
`eas build:list --json` (`artifacts.buildUrl`), unzip, then
`codesign -d --entitlements :- Payload/*.app | grep aps-environment`. A TestFlight build must read
`production`.

## Build status

**Done**
- Registration, rename (with graph repointing), delete, per-device test — API and web UI (#187).
- `EXPO_ACCESS_TOKEN` support, with a 403-with-no-token message that names the cause (#178).
- APNs key corrected to Sandbox & Production; delivery verified on iPhone and iPad.

**Pending**
- **#190** — read receipts. Today a push rejected by Apple leaves a run reading `asked <name>` →
  `expired → failure`, which is indistinguishable from a person ignoring it.
- **#188** — a deleted recipient must not resolve to a different one in the editor.
- **#187 Phase 2 half** — the delete dialog names the consequence but does not count the workflows
  that address the phone.
- **#186** — Device Gate should not offer Telegram recipients it can never be answered by.

**Not done / backlog**
- Android/FCM has not been exercised at all; every failure above was iOS.
- `push_devices.enabled` exists and is honoured by `enabled_tokens`, but nothing writes it from the
  UI — there is no way to mute a phone without unregistering it.
