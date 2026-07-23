# sentinel-mobile

The Sentinel phone app (Expo / React Native / TypeScript). Card **#139**, plan:
[`docs/plans/mobile-app.md`](../docs/plans/mobile-app.md).

**Phase 1 (this code):** sign in, register the phone as a Recipient, receive Sentinel alerts as a
native push, fire a test notification. Live (Phase 2) and Recordings (Phase 3) are labelled
placeholders.

## The one idea to hold on to

**A phone is its own Recipient.** After sign-in the app registers its Expo push token with control
under a label taken from the device name ("Jackes — iPhone"). That label shows up in the workflow
editor's **Notify Recipient** dropdown next to the Telegram beacons, so a graph can address this
exact phone. Recipient *groups* (fan out to several at once) are a later feature.

## Running it

Installed and verified on Expo **SDK 57** — `tsc`, `expo-doctor` (20/20) and
`expo export --platform ios` all pass.

```bash
cd sentinel-mobile
npm install
npx expo start
```

**`.npmrc` sets `legacy-peer-deps`** and it is load-bearing: Expo pins `react` to the exact version
the SDK ships, while a transitive `react-dom` asks for a newer one — npm treats that as fatal and
refuses to install. Do not delete it.

### Known upstream break: Expo + Xcode 26.3

`expo-modules-jsi@57.0.4` (the newest published) does not compile under Xcode 26.3's Swift type
checker — one line in `JavaScriptCodable+Date.swift` is rejected as *"type of expression is ambiguous
without a type annotation"*, and the whole iOS build fails with it. This is Expo's code, not ours.

The fix binds the magnitude to an explicit `Double` before comparing — semantically identical.
It is kept as a `patch-package` patch under `patches/` and reapplied by the `postinstall` hook, so a
fresh `npm install` stays buildable. **If a future SDK bump fixes it upstream, delete the patch** —
a stale patch that no longer applies fails the install loudly.

### EAS

The project is **`@olharai/sentinel`** (id in `app.json` → `expo.extra.eas.projectId`), owned by the
**olharai** account. `eas whoami` must show it; log in as the account that owns OlharAI.

### Before push can work

1. **A development build is required** — `expo-notifications` no longer delivers push in Expo Go on
   either platform. Either
   `eas build --profile development --platform ios` (cloud; needs Apple credentials — see card #82
   for the existing signing setup, Team `3A4H2Y6ZVM`) or a local `npx expo run:ios --device`.
2. **Physical hardware.** A simulator cannot receive push; the Alerts tab says so explicitly rather
   than looking broken.
3. Permission is requested on first launch after sign-in; if denied, the Alerts tab reports it and
   points at system settings.

### Pointing at a local backend

Defaults are production (`control.thesentinels.online`). To hit a laptop:

```bash
EXPO_PUBLIC_CONTROL_URL=http://192.168.1.50:8790 npx expo start
```

Use the LAN IP — `localhost` from a phone means the phone.

## Layout

| Path | What |
|---|---|
| `app/_layout.tsx` | Root layout + session gate (stored JWT → tabs, else login) |
| `app/login.tsx` | Email/password → `POST /auth/login`, then registers for push |
| `app/(tabs)/index.tsx` | **Alerts** — registration status, Recipient label, test push, received log |
| `app/(tabs)/live.tsx`, `recordings.tsx` | Phase 2 / 3 placeholders |
| `src/config.ts` | Control + gateway base URLs |
| `src/session.ts` | JWT in `expo-secure-store`, authenticated `api()` helper |
| `src/push.ts` | Permission → Expo token → `POST /push/devices`; test send |

## Backend it talks to

- `POST /auth/login` → `{ token, user_id, tenant_id, is_master, perms }` (HS256, 24h; the same
  token authenticates the gateway for Phase 2 live video).
- `GET|POST /push/devices`, `DELETE /push/devices/:id`, `POST /push/devices/test` —
  `sentinel-control/src/domain/push_devices.rs`, gated on `beacons.read` / `beacons.manage`.
- Delivery: `NotifyRecipient` resolves the label against `push_devices` and queues a
  `channel: "push"` job on `work_jobs`; `sentinel-notification` sends it via Expo's push service.
