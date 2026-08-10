# Devices that cannot be reached on the LAN

Type: grilling
Status: open

## Question

The edge is required and there is no cloud fallback — decided. That makes this sharp: **some devices
simply cannot be controlled locally**, and with one path they become unsupported.

At least three kinds:

- **Behind a Zigbee/BLE gateway.** The gateway is on the LAN; its children are not. Whether the
  local protocol can address a sub-device THROUGH the gateway is the thing to establish.
- **Cloud-only categories.** Some devices expose functions that only exist as cloud calls.
- **On a different subnet or VLAN from the appliance**, where UDP discovery never arrives — common
  in exactly the small-business networks this product targets.

The decisions:

- Does an unsupported device disappear from the picker, or appear disabled with a reason? Hiding it
  is cleaner; showing it disabled is the only version that answers "why is my plug missing".
- Does the sync mark each device's local reachability, so the answer is data rather than a guess at
  control time?
- Is "cloud-only for these specific devices" really out of the question? The fallback was ruled out
  as a general mechanism — a narrow, per-device, explicitly-labelled exception is a different
  proposition and may be the honest answer for gateways.

That last point may reopen a charted decision. It should be argued, not assumed away.

## What 01 established

[01](./01-protocol-client-in-rust.md) narrowed two of these and added a fourth kind:

- **Gateway children may not be lost after all.** `rustuya` claims sub-device support and `device22`
  handling, so addressing a Zigbee child THROUGH its gateway looks possible rather than
  ruled out. That is a claim to verify against real hardware before it is relied on — but it means
  the per-device cloud exception is a fallback, not the expected answer.
- **A fourth kind: the device someone else is already holding.** A Tuya device accepts only ONE TCP
  connection. Anything else on the LAN that holds it — the Smart Life app in LAN mode, a Home
  Assistant install, a second appliance — takes the device away from us, and the symptom is a device
  that simply stops answering. This one is not a property of the device but of the site, so it
  cannot be answered by a flag on the device row.
- **The likely real failure is reconnection, not capability.** Across this ecosystem the recurring
  complaint is a local connection that dropped and never recovered while the vendor app kept
  working, because the app falls back to the cloud and a local integration has nowhere to fall. With
  the edge required, we have nowhere to fall either — so recovery is a first-class requirement here,
  not an afterthought.
