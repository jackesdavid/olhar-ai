# Does the SDK build alongside what the app already links?

Type: task
Status: open

## Question

Add `ThingSmartHomeKit` to `sentinel-mobile` and get a device build onto a phone. Nothing else — no
account, no pairing, no UI. The question is only whether the binary links and runs.

This is the one thing the documentation cannot answer, and it is load-bearing: the app already
carries a custom Expo module, a **155 MB static Rust library** (`libsentinel_edge.a`) and a
**statically linked GStreamer framework**. A second large SDK with its own transitive pods is where
duplicate symbols, conflicting deployment targets, bitcode/architecture mismatches and app-size
limits actually show up.

Record in the answer:

- Whether `pod install` resolves, and what it pulled in.
- Whether the app links, launches and still starts the embedded edge — the existing native module
  must keep working, and a Tuya SDK that displaces it is a much bigger decision than an integration.
- The size delta of the built `.app`, because the Rust library alone is already large.
- Any deployment-target bump the pods force. Expo already raises the pod target to match
  ExpoModulesCore; a higher floor from Tuya would drop older iPhones, which is a product fact, not
  a build detail.

AFK where possible; the free Development edition (100 users) is enough for this — no licence is
needed to find out whether it builds.
