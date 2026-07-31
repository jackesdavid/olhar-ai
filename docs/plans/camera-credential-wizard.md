# Camera credential wizard — the camera proves the creds before we save them

**Board card:** #88 Camera credential wizard (In Progress — reworked)
**Goal:** When a user sets a camera's credentials, they should *see proof it worked* — and when it
doesn't, be told **what actually went wrong**, in terms that point at the fix. Never save unproven
credentials, and never touch the camera hard enough to break it.

Live-camera bug **B**. Sibling: [[live-badge-loading-states]] (#89, bug C).

---

## Why this was reworked (v1 shipped a camera-bricking bug)

v1 shipped as "save, then poll `snapshotBlob` every 2s for 25s; a frame means the creds work." Field
testing on a Tapo C100 (2026-07-16) turned a **correct** password into a camera that refused
*everyone* — the app, the Live tab, and even `localhost:1984` directly on the edge. Three compounding
faults:

1. **It saved before it proved.** `saveAndTest` pushed creds to the edge *first*, which rewrote
   test — it overwrote a working camera's config and started an auth storm.
   own reconnects, and retried *unconditionally* — including on hard auth failure, which is exactly
   when you must stop. Consumer cameras (Tapo, Reolink, Wyze) lock their RTSP server after such a
   burst. **The lock outlives the app and clears only on a power cycle.**
   camera — surfaced as "check your credentials". So the user re-typed *correct* creds, which
   re-armed the storm, which deepened the lock. That feedback loop is the whole bug.

The tell, found by probing the camera directly: a locked Tapo answers **`RTSP/1.0 400 Server is
locked`** to an `OPTIONS` request carrying **no credentials at all**, and returns the identical 400
for a deliberately wrong password. A camera actually judging creds answers `401`. So "locked" and
"wrong password" are cleanly separable on the wire — v1 just never asked.

**This note's own Open decision #1 predicted this and guessed wrong:** it chose "generic
couldn't-connect copy" over threading a reason code from the edge. The generic copy *was* the bug. A
verdict the user can't act on is worse than no verdict — it sends them at the wrong fix, and here the
wrong fix (retry) is what causes the damage.

## The shape now: test-then-save, one attempt, typed verdict

- **`crates/app-core/src/rtsp_probe.rs`** (new) — a pure-Rust RTSP probe. `OPTIONS` (unauthenticated,
  so its answer is about the *server*, catching a lockout without spending an auth attempt) then one
  `DESCRIBE` answering the digest/basic challenge. Returns `ProbeStatus`:
  `ok | auth_failed | locked | not_found | unreachable | timeout | error`.
  **One connection, at most two requests, never a retry** — `ProbeStatus::is_retryable()` exists to
  make "don't retry a rejection" enforceable rather than a comment.
- **Edge `POST /probe/:ip`** (`sentinel-edge/src/http.rs`) — probes with the supplied creds,
  via `snapshot_from_rtsp` as visual proof (base64 inline). A camera with no cached RTSP URL yet is
  resolved over ONVIF first, whose rejection is itself an `auth_failed` verdict.
- **Control `POST /devices/:ip/probe`** (`sentinel-control/src/domain/devices.rs`) — proxies to the
  tenant's box, `devices.write`-gated, 30s bounded. Creds ride the request and are dropped; they are
  **not** staged in `pending_creds` and never hit the DB (CLAUDE.md invariant holds).
- **App** (`data.ts` `probeCredentials`, `routes/devices/+page.svelte`) — probe → on `ok` only, save
  the name + `setCredentials`. Each status maps to its own headline + advice via `verdictTitle` /
  `verdictAdvice`; `canRetry()` hides "Test again" for `auth_failed`/`locked`, because offering the
  retry is offering the lockout.

Also fixed en route: **`rtsp_url_with_creds` didn't percent-encode**, so a password containing `@`
or `:` silently corrupted the URL and a correct password came back as an auth failure.

## Verification

- Correct creds → frame + "Camera verified"; creds saved only now. *(Blocked on the C100 lockout
  clearing — needs a power cycle.)*
- Wrong creds → "The camera rejected these credentials", **nothing saved**, no retry button, the
  previously-working config untouched.
- Locked camera → "The camera has locked its video stream… unplug it for ~30 seconds", **not** a
  credential complaint. Reproducible today against the C100 at 192.168.1.184.
- Unit tests (`cargo test -p app-core rtsp_probe`, 7 passing) cover URL parsing with `@` in the
  password, userinfo encoding, status-line parsing, the 400-means-locked rule, digest against the
  RFC 2069 worked example, basic fallback, and that hard failures are non-retryable.

## Open decisions (resolved)

1. **How precise can the failure reason be?** — v1 guessed "generic" and that was the bug. **Resolved:
   thread a typed status from the edge.** It's not just cheap, it's the point: the causes need
   *opposite* user actions (retype vs power-cycle vs check the network).
2. **Poll or push?** — v1 polled. **Resolved: neither.** One synchronous probe. The poll was the
   lockout vector.
3. **Snapshot via control proxy or gateway?** — control, unchanged.
4. **Persist the verified snapshot as a thumbnail?** — still ephemeral; no cloud storage of frames.

## Out of scope (v1)

- A full multi-camera onboarding wizard (this is per-camera verify).
- Persisting snapshots/thumbnails server-side.
- Changing where creds live (still edge-only, staged-once).
- Auto-recovering a locked camera (some expose an ONVIF `SystemReboot`; a power cycle is the reliable
  answer, and rebooting a user's camera unasked is not ours to do).
