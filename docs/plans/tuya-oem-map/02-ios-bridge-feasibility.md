# What the iOS bridge actually requires

Type: research
Status: resolved

## Question

Tuya's official React Native wrapper is decommissioned; the documented path is to bridge to the
native SDK yourself. Establish what that means for THIS app before anyone commits to it.

- Which iOS SDK is the right one for our case (Smart Life App SDK vs the smaller device-control
  SDKs), what it ships as (CocoaPods? xcframework?), and how it is initialised.
- Whether it coexists with what the app already links. `sentinel-mobile` is Expo with a prebuilt
  `ios/` project, a custom Expo module, a **155 MB static Rust library**, and a statically linked
  GStreamer framework. A second large SDK with its own dependencies is exactly where symbol
  conflicts and binary-size limits show up.
- What the pairing APIs look like for Wi-Fi EZ and AP mode specifically: what the app must supply
  (SSID, password, token), what callbacks it gets, and how failures are reported. Pairing is the
  part users experience as "it didn't work", so its error surface matters more than its happy path.
- Whether an account can be created and signed in **programmatically** — the invisible-account
  decision depends on it. If the SDK insists on its own login UI, that decision needs revisiting.

That last point can invalidate a charted decision, which is why it is research and not an
assumption.

## Answer

**The charted decision survives, and the specific thing that could have killed it does not.**

- **Distribution: CocoaPods** — `ThingSmartHomeKit`, with `ThingSmartBusinessExtensionKit` as the
  recommended superset. The app already has a prebuilt `ios/` with a Podfile, so this is the
  dependency mechanism it uses, not a new one.
- **Programmatic sign-up: YES**, and by the route that matters. Email registration exists but
  requires a verification CODE — which would have broken "invisible" outright, since the user would
  have to read an email from a vendor they are not supposed to know about. The path that works is
  **`registerAnonymousWithCountryCode:userName:success:failure:`**: an anonymous account, no code,
  no email. Tuya also documents upgrading an anonymous account to a real one later by adding an
  email and resetting the password — a door left open rather than a decision made now.
- **Repository:** `tuya/tuya-home-ios-sdk`, with Objective-C and Swift sample apps.

So the risk flagged while charting — *"if the SDK insists on its own login UI, the invisible-account
decision reopens"* — is closed. It does not.

### What this does NOT answer, and should not pretend to

Documentation cannot tell us whether the SDK **coexists with what this app already links**:
`sentinel-mobile` carries a custom Expo module, a **155 MB static Rust library** and a statically
linked GStreamer framework. Adding a second large SDK with its own transitive pods is exactly where
duplicate symbols, conflicting minimum deployment targets and binary-size limits surface — and none
of that is discoverable by reading. It needs a build.

That is a different kind of work, so it is a new ticket rather than a loose end here: see
[Does the SDK build alongside what the app already links](./06-sdk-coexistence-build.md).

Also unestablished, and deliberately left for when the SDK is actually in hand: the exact Wi-Fi
EZ/AP pairing call shape and its failure modes. Pairing is what users experience as "it didn't
work", so its error surface deserves to be learned from the API, not from a doc index.

Sources: [tuya/tuya-home-ios-sdk](https://github.com/tuya/tuya-home-ios-sdk) ·
[Register and Login with Anonymous Account](https://developer.tuya.com/en/docs/app-development/usertourist?id=Ka6a99lylyces) ·
[User Account Management (iOS)](https://developer.tuya.com/en/docs/app-development/user?id=Ka5cgmm97jlt2) ·
[Quick Start for iOS](https://developer.tuya.com/en/docs/app-development/tutorial-for-ios-final?id=Kaohsbl43lxjb)
