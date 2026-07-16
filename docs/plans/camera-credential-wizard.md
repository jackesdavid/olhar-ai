# Camera credential wizard — prove the creds work with a live snapshot

**Board card:** #88 Camera credential wizard (In Progress)
**Goal:** When a user adds or fixes a camera's credentials, they should *see proof it worked* — a
snapshot from the camera — instead of saving into a void and wondering whether it's right. Today
there is no signal at all until (maybe) a stream eventually appears.

Live-camera bug **B**. Sibling: [[live-badge-loading-states]] (#89, bug C). Depends on the same edge
snapshot path that already works.

---

## Why (the gap today)

- Credential entry lives in the Devices detail dialog (`sentinel-app/src/routes/devices/+page.svelte`):
  `saveDetail` saves the name, then `data.setCredentials(ip, user, pass)` →
  `POST /devices/:ip/credentials` (`sentinel-app/src/lib/data.ts:132`). Control **stages the creds in
  memory** (`st.pending_creds`, `sentinel-control/src/domain/devices.rs:76-87`) and hands them to the
  edge **once** on its next config poll; the edge writes them to its cache and regenerates go2rtc.
- **There is no confirmation.** `testCredentials` / `captureSnapshot` / `validateDevice` are all
  `cloudOnly(...)` stubs that throw (`data.ts:157-163`). The only eventual signal is the async
  "streaming" list (`GET /devices/streaming`) driving the Online dot — which is slow and indirect.
- **The proof already exists and works:** the snapshot path `data.snapshotBlob(ip)` →
  `GET /devices/:ip/snapshot` (control, `devices.rs:151-190`) → edge `:8080/snapshot/:ip`
  (`sentinel-edge/src/http.rs`) → go2rtc `frame.jpeg` (`crates/app-core/src/go2rtc.rs:108-128`). A
  frame only comes back if the RTSP credentials authenticated — **a returned snapshot IS proof the
  creds are correct.** It's just not wired into the credential flow (only the workflow editor uses it).

## The wizard flow

Replace "type user/pass, click Save, hope" with a short guided sequence, in the Devices detail
dialog (or a dedicated step):

1. **Enter** camera username + password (IP is already known from discovery).
2. **Save + test** (one action): `setCredentials`, then enter a **testing** state.
3. **Wait for the edge to apply them.** The creds reach the edge on its next config poll (~1s, the
   `bump(tid)` wakes the long-poll), which regenerates go2rtc. Poll `snapshotBlob(ip)` with backoff
   for up to ~20-25s (go2rtc needs a moment to connect the RTSP stream).
4. **Show the result:**
   - **Snapshot renders → "Camera verified"** — show the frame, big and clear. This is the proof.
   - **Timeout / snapshot 5xx → "Couldn't connect."** Distinguish the likely causes from the edge's
     error where possible (auth failure vs unreachable vs no RTSP profile) — see Open decisions.
5. The snapshot doubles as a **thumbnail** the Devices list can keep showing.

## Phase 1 — un-stub the plumbing

- `data.ts`: make `captureSnapshot`/the test call real — reuse `snapshotBlob(ip)` (it already goes
  app → control → edge → go2rtc). Drop the `cloudOnly` stubs.
- A small `testCredentials(ip, user, pass)` helper: `setCredentials` then poll `snapshotBlob` until a
  frame or timeout; return `{ok, snapshotUrl}` or `{ok:false, reason}`.
- Control: `GET /devices/:ip/snapshot` already exists and is `devices.read`-gated. Confirm it returns
  a clean 5xx (not a hang) when the edge/go2rtc has no frame yet, so the poll can distinguish
  "not ready" from "failed". Consider a lightweight `?probe=1` that fails fast.

## Phase 2 — the wizard UI

- In the Devices detail dialog: a **Test** button next to the credentials, and a result area showing
  the spinner → snapshot → verified/failed. Named helpers for the state (house style, no markup
  ternaries).
- Plain-language failure copy: "The camera rejected these credentials" vs "Couldn't reach the camera
  at 192.168.1.x" — the operator needs to know *which* to fix.
- On success, keep the snapshot as the device thumbnail.

## Phase 3 — first-run "Add camera" path

- The same test step belongs in the initial add-camera flow (discovery → set creds → **verify**), so
  a camera is never "added" in an unknown state. Route both through the same wizard component.

## Verification

- Enter correct creds → within ~20s a snapshot renders + "Verified". (Real camera on the test box.)
- Enter wrong creds → clear "credentials rejected", no snapshot, no false "Verified".
- A credential-less camera (go2rtc skips it, `go2rtc.rs:66-72`) → the test explains it needs creds.
- The snapshot shows through the control proxy (no direct edge/gateway exposure needed).

## Open decisions (grill before building)

1. **How precise can the failure reason be?** The edge's `capture_snapshot_bytes` falls back
   ONVIF→ffmpeg; go2rtc auth failures vs unreachable-host vs no-RTSP-profile may not be cleanly
   distinguishable from a JPEG 500. Decide whether v1 says a generic "couldn't connect — check
   credentials and that the camera is reachable," or we thread a reason code from the edge. Leaning:
   thread a coarse reason (`auth` | `unreachable` | `no_stream`) from the edge if cheap, else generic.
2. **Snapshot via control proxy or the gateway?** `snapshotBlob` uses control (`:8790` → edge `:8080`).
   The gateway can also serve `/api/frame.jpeg`. Control is simpler and already wired; keep it.
3. **Poll or push?** A 1-2s poll for ~20s is simple and the window is short. SSE is overkill here.
4. **Store the verified snapshot** as a persisted thumbnail (where?), or keep it ephemeral per-session?
   Leaning ephemeral in v1 (no cloud storage of camera frames), revisit if a thumbnail grid wants it.

## Out of scope (v1)

- A full multi-camera onboarding wizard (this is per-camera verify).
- Persisting snapshots/thumbnails server-side.
- Changing where creds live (still edge-only, staged-once — CLAUDE.md invariant).
