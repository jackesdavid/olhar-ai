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
