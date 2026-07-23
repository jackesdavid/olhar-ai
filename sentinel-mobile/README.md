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

This directory has never been installed or launched — the dependency versions in `package.json` are
deliberately loose and **must** be aligned to the SDK before the first run:

```bash
cd sentinel-mobile
npm install
npx expo install --fix     # pins every expo/react-native package to the SDK's versions
npx expo start
```

### Before push can work

1. **`eas init`** (or reuse an existing EAS project) and put the real id in `app.json` →
   `expo.extra.eas.projectId`. Without it `getExpoPushTokenAsync` throws; the app reports that
   explicitly rather than failing silently.
2. **Run on physical hardware.** A simulator cannot receive push; the Alerts tab says so.
3. **iOS** additionally needs a development build (`npx expo run:ios` or an EAS build) — push does
   not work in Expo Go on iOS.

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
