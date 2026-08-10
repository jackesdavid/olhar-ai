# Tuya local control [map]

## Destination

Every decision locked for controlling Tuya devices **from the edge over the LAN**, with the cloud
out of the execution path — so implementation can start without stopping to ask. Not the built
integration: the point at which nothing is left to decide.

## Notes

**Domain:** today `run_tuya_control` executes in `sentinel-flow` and calls the Tuya Cloud API with
the tenant's HMAC-signed credentials. Local control inverts that: flow sends the command to the edge
over the SSE channel, and the edge speaks the Tuya LAN protocol directly to the device.

```
today:  flow ──HTTPS──► Tuya Cloud ──► internet ──► device
after:  flow ──SSE────► edge ──LAN──► device
```

**Read [[tuya]] first** — its "Local control (future — #133)" section already has the shape right:
the Cloud API returns each device's `local_key`; with `id + local_key + LAN ip + protocol version`
the edge can control it offline. The WAN ip the cloud returns is useless, so the edge finds the
device itself on UDP 6666/6667. `local_key` is a SECRET and lives on the edge like camera
credentials, never in the cloud DB.

Decided while charting, do not reopen:
- **The edge becomes REQUIRED for Tuya.** No cloud fallback for control — one path, not two. A
  tenant without an appliance loses Tuya, and that is a deliberate commercial choice.
- The node, the name selector and the ports do not change. A customer's existing graph keeps
  working; only what happens behind `run_tuya_control` moves.

**What this does NOT remove:** the BYO onboarding. The `local_key` still comes from the Cloud API,
so the five-step setup stays exactly as it is. Local control takes the cloud out of the EXECUTION
path, not the CONFIGURATION path — worth being precise about, because "local" invites the other
reading.

**Proven elsewhere:** tinytuya and LocalTuya do this, across protocol versions 3.1–3.5.

**Skills:** `/grilling` and `/domain-modeling` on conversation tickets. `/research` runs AFK.

**Standing preference:** decisions, not deliverables.

## Decisions so far

<!-- one line per resolved ticket -->

- **The protocol client is the `rustuya` crate, not a port** — 3.1–3.5, MIT, pure Rust on tokio, no
  C dependency. Days rather than weeks. Young and single-maintainer, so vendor-or-fork if it goes
  quiet; the fallback is the port we would otherwise have started with.
  ([[01-protocol-client-in-rust]])
- **One persistent connection per device, heartbeated** — the device permits only ONE TCP connection
  and closes an idle socket at ~30s. This is the camera ONE-PIPELINE rule on different hardware, and
  it fails the same silent way. 3.4/3.5 negotiate a per-connection session key, so dial-per-command
  would also pay three round-trips per command. ([[01-protocol-client-in-rust]])
- **Discovery listens on 6666, 6667 AND 7000** — the broadcast carries device id, ip and protocol
  version, so the edge learns the version rather than being told it. 7000 is *solicited*: listening
  only on 6666/6667 silently misses the newest hardware. ([[01-protocol-client-in-rust]])

## Not yet specified

- **What the Live/status path does.** `Read Tuya` fetches device state; whether it also moves local,
  polls, or subscribes to the device's own pushes depends on what the protocol client turns out to
  support.
- **Whether the edge should announce device reachability.** The Devices page shows camera status
  from real frames; a Tuya device that has silently dropped off the LAN deserves the same honesty,
  but the shape depends on the discovery decision.
- **Migration for the tenant already using cloud control.** Sharp only once we know whether local
  works for their specific devices.

## Out of scope

- **Cloud fallback for control** — ruled out with the decision that the edge is required. Two paths
  is the complexity this avoids, and it is the same argument that killed the OEM route's appeal.
- **Replacing device firmware** (Tasmota/ESPHome) — total independence, and unusable as a product:
  post-2020 chips closed the OTA exploit, so it means opening the device and soldering.
- **Matter** — the durable standard and fully local, but it covers only newer hardware and is a
  stack of its own. A separate effort if and when the device mix justifies it.
