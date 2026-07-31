# Camera status visibility — Implementation Plan

**Board card:** #155 Camera status — show WHICH stage failed, per camera
**Goal:** a camera's row answers "is this working, and if not, which stage failed and what did the
camera say" — without anyone opening a terminal.

## Why

Asked "why is my camera not working", the honest answer today is *we cannot tell you*. On
2026-07-30 the diagnosis took a shell on the edge box and two commands:

```
ping 192.168.0.7   →  100% packet loss
nc -z 192.168.0.7 554  →  UNREACHABLE
```

The product said nothing. The UI showed a camera with `has_stream: true`; the frame endpoint
returned a 502 whose body — "the camera's decoder produced nothing" — describes a symptom three
layers below the cause and names neither the stage nor the fix.

**The diagnosis already exists and is thrown away.** `app_core::rtsp_probe::ProbeStatus` already
distinguishes every stage worth distinguishing:

| Status | Means | What the user should do |
|---|---|---|
| `Unreachable` | no TCP — wrong IP, camera off, firewall | check power/network |
| `Timeout` | connected, camera never answered | camera wedged; power-cycle |
| `AuthFailed` | camera evaluated creds and refused | re-enter credentials |
| `Locked` | refusing ALL clients regardless of creds | power-cycle; stop retrying |
| `NotFound` | authenticated, no stream at that path | wrong stream path |
| `Ok` | stream describable | — |

It runs **once**, in the credential wizard, via `POST /probe/:ip` — which takes credentials in the
body, so it answers "are these creds valid", not "how is this camera right now". Nothing is stored:
`devices` has **no** health columns at all.

## Phase 1 — persist per-camera state

Migration: `devices` gains `probe_status text`, `probe_detail text`, `probe_at text` (RFC3339),
`last_frame_at text`. Today the table is `ip, xaddr, name, manufacturer, model, username, password,
rtsp_url, tenant_id, stream_mode` — nothing about whether any of it works.

## Phase 2 — the edge re-probes and reports

A periodic per-camera probe on the edge, pushed up with the existing config heartbeat.

- **Respect `ProbeStatus::is_retryable()`.** `AuthFailed` and `Locked` are hard no's — retrying is
  what *causes* the lockout. Only `Timeout`/`Unreachable` are worth re-checking.
- **Do not open a second connection.** See the ONE-PIPELINE invariant in `capture.rs`: consumer
  cameras cap concurrent RTSP sessions at 2–4. A status probe that dials again is how a working
  camera goes dark. Prefer deriving status from the existing pipeline's state (last frame time, last
  bus error) and only cold-probe a camera that has NO pipeline running.

## Phase 3 — surface it

Devices list + camera detail: the stage, the camera's own words, when it was last checked, and the
action. "Can't reach 192.168.0.7 on the network — check it's powered on and on the same LAN
(last checked 30s ago)" instead of a spinner or a stale `has_stream: true`.

## Test fixture, available now

Camera `192.168.0.7` ("Sala", Tapo C100) is physically disconnected — ground truth is known. It
must read **Unreachable**, not "streaming", and not a blank. It is the ONLY device in this tenant;
the `192.168.10.x` cameras belong to a different tenant and appliance entirely.

## Related gaps found the same day (same root cause: no telemetry)

1. **Edge logs stopped landing in Postgres on 2026-07-26** — RESOLVED, and never a fault: that is
   when Datadog was enabled, and the queue ships there and returns before the Postgres insert. The
   real problem was the VIEWER, which kept reading the table and served five-day-old data with
   nothing marking it stale. Reader removed; the write path and its Postgres fallback stay.
2. **`disk_pct()` measures the wrong volume on macOS** — it picks `/`, which is the sealed read-only
   system volume. The box reported `disk=0%` while `/System/Volumes/Data` was 100% full.
3. **`edge:full-disk` has never fired**, including on a box genuinely at 99%. It watches the
   recording quota, not free space — so it stays silent exactly when the disk fills from anything
   else.

## Verified in production — 2026-07-31

Cloud half is live (commits `efb8509`, `36afa67`, `de62071`; edge `0.5.67` published):

- migration 0083 applied; all four columns present on `devices`
- the tenant's camera starts `NULL` → the UI reads "Not checked yet", and **zero** `camera_*` events
  fired on first deploy. This is the transition guard working: `None` previous status is not a
  transition, so a fresh deploy does not announce every camera as having "come online".
- `camera_offline` / `camera_online` present in the deployed control binary
- `/edge-latest?platform=macos` → `0.5.67` with a sha256 matching the uploaded package; the
  no-platform form answers `0.5.65`, the conservative minimum across platforms
- all 10 status labels present in both locales

Not verifiable from the cloud: a camera reading `unreachable`, and the offline→online flip. Both
need the box on `0.5.67` — until then every camera reads "Not checked yet".

### `NotOnThisBox` is declared but never produced

`ProbeStatus::NotOnThisBox` exists with a label ("On a different network from this box"), and
nothing constructs it. The edge derives status only from its OWN pipelines, and control never
synthesizes it either — so the variant is dead.

It is dead rather than urgent. An earlier draft of this note claimed the four `192.168.10.x`
cameras proved the gap by sitting at "Not checked yet" forever. They do not: they belong to a
DIFFERENT TENANT (`luciusbsd`) and a different appliance, so they are correctly invisible to this
box and this tenant. That claim came from a `SELECT ... FROM devices` with no tenant filter — the
one mistake multi-tenant code exists to prevent, made while reasoning about it.

Whether the variant is worth keeping is now an open question rather than a known hole: a camera
that is genuinely in a tenant but assigned to another of that tenant's boxes is the only case it
would serve, and no such case has been observed.

## Out of scope (v1)

- Fixing the log relay (1) — its own card; this one must not depend on it.
- Historical status (a timeline of camera state) — current state first.
