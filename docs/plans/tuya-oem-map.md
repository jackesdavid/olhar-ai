# Tuya OEM in our own app [map]

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

## Not yet specified

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
