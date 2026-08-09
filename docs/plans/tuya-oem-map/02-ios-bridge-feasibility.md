# What the iOS bridge actually requires

Type: research
Status: open

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
