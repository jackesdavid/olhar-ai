# Add a camera by hand — a journey for when discovery can't

**Board card:** #97 Add camera manually — IP + ONVIF port, not just discovered (In Progress)
**Goal:** A camera you can reach but can't *discover* should still be addable. Enter its IP, ONVIF
port and credentials; the edge resolves the stream the same way it does for a discovered one.

Prompted 2026-07-17: a wifi change moved the test camera .184 → .187. It's fully reachable (RTSP +
ONVIF open, same subnet as the box) but **WS-Discovery multicast gets zero responders** — the new AP
drops multicast between wireless clients. Discovery is the *only* way to add a camera today, and it
died exactly when the one channel it needs died. A reachable camera became unusable. Per the standing
rule — *"the edge should work or be fixed by the journeys"* — the missing journey is the bug.

---

## Why this is mostly frontend

The backend already streams cameras it never discovered. `resolve_rtsp` (`sentinel-edge/src/state.rs`)
gates on **creds + a target**, nothing else:

```rust
let target = if d.xaddr.trim().is_empty() { d.ip.clone() } else { d.xaddr.clone() };
```

And `upsert` (`sentinel-control/src/domain/devices.rs`) accepts any `Device` — discovery is not a
precondition, it's just the usual *source* of one. So the only missing piece is a way for a human to
supply what discovery normally fills in: `ip`, `xaddr`, `name`.

## What discovery provides, and how a human supplies it

| Field | From discovery | By hand |
|---|---|---|
| `ip` | WS-Discovery | typed |
| `xaddr` (ONVIF endpoint) | the probe response | built: `http://{ip}:{port}/onvif/device_service` |
| `name` | camera's ONVIF name | typed (default "Camera {ip}") |
| `id` (stable ONVIF id) | `EndpointReference` | **left empty** — `Device.id`'s own doc says it
  backfills "on the next scan by IP match", so once multicast returns, reconciliation adopts it |
| creds | never (set separately) | typed, sent via the existing credentials endpoint |

The one thing a user must know that discovery knew for them: **the ONVIF port**. Default **2020**
(Tapo), offer **80** (the ONVIF norm) and **8000/8080** as common alternates. Most people won't know
it — so the verify step has to make a wrong guess obvious, not silent.

## Flow (reuses #88 end to end)

1. **"Add camera manually"** on the Devices page, beside the discovered list.
2. Fields: **Name, IP, ONVIF port (default 2020), Username, Password.**
3. Build `xaddr = http://{ip}:{port}/onvif/device_service`, `saveDevice` with an empty `id`.
4. **`probeCredentials(ip, user, pass)`** — the #88 probe. For a device with no cached RTSP URL it
   resolves via ONVIF from `xaddr`, so it validates the port, the creds, and returns a proof frame in
   one shot. This is exactly why #88's probe was built to fall back to ONVIF.
5. On `ok` → `setCredentials`, the camera is live. On `auth_failed`/`unreachable`/etc → the #88 verdict
   copy already says which to fix; a wrong ONVIF port surfaces as `unreachable`/`error`, so add one
   line to that path: "double-check the ONVIF port".

No new endpoints. The manual path and the discovered path converge at `saveDevice` + the probe.

## Out of scope (v1)

- A raw-RTSP-URL camera (no ONVIF at all). Real, but a different shape — the whole pipeline resolves
  through ONVIF today. Note it; don't build it here.
- Subnet scanning / "find cameras by pinging the range" as a discovery fallback. Bigger, and the
  manual add already unblocks the reported case.
- Auto-detecting the ONVIF port by trying the candidates. Nice, but the probe already tells the user
  a wrong port failed; try it only if the manual entry proves annoying.

## Verification

- The reported situation: camera at .187, discovery blind. Add by hand (name Hall, IP .187, port
  2020, master/anneanjo2) → verified frame, camera streams, recording graph works against it.
- Wrong port → a verdict that points at the port, not a silent failure.
- When multicast returns, a Rescan reconciles the hand-added camera's empty `id` by IP match — no
  duplicate.
