# Tauri v2 mobile — one UI codebase for web and phone

**Board card:** #202 Tauri v2 mobile — share the web app instead of maintaining a second UI
**Goal:** Ship the phone app as the **same SvelteKit application** the web runs, inside a Tauri v2
shell, so a screen is written once. Retire the parallel React Native UI.

## Why this is viable now (it was not obviously so)

Two objections were raised against it, and research on 2026-08-21 killed both:

| Objection | Reality |
|---|---|
| "Tauri has no remote push" | **False.** Several maintained v2 plugins do APNs + FCM: `tauri-plugin-mobile-push` (explicit AppDelegate delegation, no swizzling — swizzling-based approaches break on iOS 26+), `tauri-plugin-notifications`, `tauri-plugin-remote-push`, `tauri-plugin-fcm`. |
| "`getUserMedia` does not work in WKWebView" | **False since iOS 14.3.** Apple enabled it; iOS 15 added `setCameraCaptureState`. Camera needs a WKWebView UI delegate to grant the permission prompt, not a native rewrite. |

Both were true once. Neither is true now, and both were repeated from memory rather than checked —
worth remembering the next time this decision gets re-litigated.

## What actually makes this attractive

- **`sentinel-app` is SvelteKit with `adapter-static`.** It already builds to a static bundle, which
  is exactly what a Tauri shell loads. No SSR to unwind.
- **The design-parity tax disappears.** Today every token is mirrored into
  `sentinel-mobile/src/styles` by hand, and the rule is that the app must match the web exactly.
  One codebase makes that structural instead of a discipline.
- **Tauri is Rust, and `sentinel-edge` is Rust that already cross-compiles for iOS arm64** (#144).
  Today it is bolted in through a podspec with static GStreamer plugins. In a Tauri shell it is a
  crate dependency. This is the strongest single argument and it has nothing to do with the UI.
- The mobile UI is only **34 files / ~4,450 lines**, so the thing being replaced is small.

## What it costs

- **Four native capabilities need plugins**, each a real integration: remote push, camera, keychain
  (`expo-secure-store` → a Tauri store/keychain plugin), biometrics (`expo-local-authentication`).
- **The push plugins are third-party.** That is an ordinary supply risk, but push is the channel we
  just spent #190 making verifiable — a regression there is expensive and hard to see.
- **#185 (App Store review readiness) is in flight.** Swapping the shell restarts that clock. This
  card should NOT start before #185 lands.
- The app is already on TestFlight rails (once `ASC_APP_ID` is set); a Tauri build needs its own
  signing and upload path in CI, and the no-local-builds rule applies to it too.

## Phases

### Phase 1 — prove the shell (throwaway)
A Tauri v2 app that loads the existing `sentinel-app` static build and logs in against production.
Nothing native. The question it answers: does the real UI feel right on a phone, and does anything
in it assume a desktop browser? Cheap, and it either kills the idea or de-risks everything after.

### Phase 2 — the four capabilities, one at a time
Push first (highest risk, and #190 gives us receipts to verify it end to end), then camera via
`getUserMedia` with the WKWebView permission delegate, then keychain, then biometrics. Each proven
on a device before the next starts.

### Phase 3 — the edge as a crate
Link `sentinel-edge` directly instead of through the podspec. This is the payoff that no amount of
React Native work can reach.

### Phase 4 — cut over
Ship both, compare, retire `sentinel-mobile`. Not before the App Store build is stable.

## Verification criteria

1. A screen written once renders on web and phone with no mirrored styles.
2. A push sent from a workflow arrives on the phone, and #190's receipt confirms it.
3. Face capture works from the phone camera through `getUserMedia`.
4. `sentinel-edge` runs in-process with no podspec.
5. The App Store build is produced in CI, not on a laptop.

## Out of scope
- Android. iOS first; the shell is the same, the plugins are not.
