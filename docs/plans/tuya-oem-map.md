# Tuya OEM in our own app [map] — PARKED

> **Parked 2026-08-09 on cost, with the number in hand.** USD 5 000 the first year and USD 2 000/yr
> after, mandatory before a single customer is served, for an integration that already works by
> another route. The map is left standing rather than deleted: the research is durable, and the
> question returns the moment the economics change.
>
> **What would reopen it**, concretely — so this is a decision and not a mood:
> - Tuya opening OAuth to non-OEM apps, which removes the licence from the path entirely.
> - Enough tenants on Tuya that ~USD 170/month is noise rather than a bet. At a handful of tenants
>   it is the whole integration's margin; at a hundred it is a rounding error.
> - Selling hardware, where owning the pairing flow is the product rather than a convenience.
>
> **What stays true regardless:** BYO onboarding is five steps and four happen outside our product.
> That is the real pain, and it is fixable without a licence — the console deep link shipped on
> 2026-08-09 is the first piece.


## Destination

Every decision locked for embedding Tuya's App SDK **inside Olhar AI** — replacing BYO-project — so
implementation can start without stopping to ask. Not the built integration: the point at which
nothing is left to decide.

## Notes

**Domain:** today each tenant brings their OWN Tuya Cloud project and pastes Access ID/Secret
([[tuya]]). It works and is dogfooded, but onboarding is five steps and four of them happen outside
our product. The OEM route puts account, pairing and control inside our app instead.

**Read [[tuya]] first.** It holds the credential model, the HMAC signing, the node/catalog shape,
and — importantly — the reasons the self-service alternatives were each ruled out. This map does not
relitigate those; it charts what the OEM decision opens up.

Decided while charting, do not reopen:
- The **SDK embedded in Olhar AI**, not a Tuya-generated white-label app. One app, our experience.
- **OEM REPLACES BYO.** A single path, not two — two ways for a device to arrive is two ways for it
  to break.
- **iOS first.** The bridge is written once per platform and there is no Android app today.
- **The Tuya account is invisible**, created under the Olhar AI login. The user never learns Tuya
  exists.
- **Pairing v1 is Wi-Fi only** (EZ + AP): plugs, switches, bulbs — what the product controls today.

**Two facts already established, worth not rediscovering:**
- Tuya's official React Native SDK repo is **decommissioned**; bridging to the native SDK is the
  documented path now.
- Smart Life and an SDK app have **separate account databases** — a user's existing devices do not
  appear, they must be re-paired. This was accepted knowingly.

**Skills:** `/grilling` and `/domain-modeling` on every conversation ticket. `/research` runs AFK.

**Standing preference:** decisions, not deliverables.

## Decisions so far

<!-- one line per resolved ticket -->

- [What the OEM licence actually costs](./tuya-oem-map/01-what-the-licence-costs.md) — USD 5 000
  first year then 2 000/yr, mandatory to ship on the App Store; the free tier caps at 100 users and
  is non-commercial, which is enough to BUILD and prove the whole thing without paying.
- [What the iOS bridge actually requires](./tuya-oem-map/02-ios-bridge-feasibility.md) — CocoaPods
  (`ThingSmartHomeKit`), and anonymous sign-up with NO verification code exists, so the invisible
  account holds. Whether it coexists with our Rust lib and GStreamer needs a build, not a doc.

## Not yet specified

- **The Wi-Fi EZ/AP pairing call shape and its failure modes.** Pairing is what users experience as
  "it didn't work", so its error surface should be learned from the API with the SDK in hand rather
  than from a documentation index.
- **App Store review and privacy.** A pairing flow wants local network and Bluetooth permissions and
  ships a third-party SDK that phones home. What Apple asks for, and what the privacy labels must
  say, only gets sharp once the SDK surface is known.
- **What becomes of the BYO code and `tuya_config`.** Deleting it is easy; deciding when, relative
  to the last migrated tenant, is not — and it hangs on the migration decision.
- **How this meets local control (#133).** Controlling a device from the edge, without the cloud
  round trip, was already a backlog idea. Whether OEM helps or hinders it is unclear until the
  server-side control question is settled.

## Out of scope

- **The Tuya-generated OEM app** — a white-label Smart Life clone that Tuya builds. Cheaper in
  engineering and rejected deliberately: it means the customer installs a second app, which is the
  problem this effort exists to remove.
- **Android** — not in this effort. There is no Android app to embed into, and adding one is its own
  undertaking rather than a platform of this one.
- **BLE pairing** — deliberately out of v1. Some devices are BLE-only and will simply not be
  supported at first; that is a known gap, not an oversight.
