# Mobile App (minimal) — Implementation Plan

**Use case:** _(none yet — new surface; this plan is the spec)_   **Board card:** #139 Mobile app (push + live + recordings, edge-ready)
**Goal:** Ship a minimal cross-platform mobile app that (1) receives Sentinel notifications as a first-class Recipient channel alongside Telegram, (2) shows live camera feeds like the Live Tab, (3) plays recordings — and lays the groundwork for the phone to later act as an edge (its own camera as a transparent camera to the platform).

**Stack decision:** **React Native + Expo** (chosen). One codebase iOS+Android, reuses the team's TS knowledge from `sentinel-app`, `expo-notifications` gives FCM+APNs push out of the box, OTA updates. The eventual phone-as-edge module is native (Rust via UniFFI) — a known path, deferred to Phase 4.

---

## Architecture fit (grounded)

- **Auth is already mobile-friendly**: HS256 JWT bearer shared between control (`:8790`) and gateway (`:8789`). `POST {control}/auth/login → {token}`; Google OAuth also exists. Claims carry `perms[]`; `live.view` is Admin-only by default (`sentinel-control/src/rbac.rs`). A native client sends `Authorization: Bearer` to control, and either `?token=` or a bearer header to the gateway WS.
- **Recipients today = Telegram-only.** `NotificationTarget { label, chat_id, enabled }` (`crates/app-core/src/notification/domain.rs`), table `notification_targets`, CRUD at `sentinel-control/src/domain/beacons.rs`. There is **no channel/type field** and **no push code anywhere** (no FCM/APNs/VAPID). Sends go through the `Notifier` port (`crates/app-core/src/ports.rs:66`, Telegram-shaped) → `work_jobs` `notification/send` → `sentinel-notification/src/consumer.rs` (always Telegram). Node kinds `GetRecipient`/`NotifyRecipient` are already channel-agnostic at the graph level (they carry a `Recipient` label) — reusable once recipient resolution learns the channel.
- **Live = MSE (fragmented MP4) over WebSocket** through `sentinel-gateway` → edge go2rtc over the tailnet. Stream name `cam_<ip-with-underscores>` (+`_hd`). Live Tab is a hand-rolled MSE player (`sentinel-app/src/routes/live/+page.svelte`). Not WebRTC.
- **Recordings = two-tier.** Video on the edge (source of truth) + durable copies in Cloudflare R2 (`crates/app-core/src/clip_store.rs`). Control stores metadata only. `GET /recordings` (list), `GET /recordings/:id/url` (presigned R2 GET, TTL 3600s), `GET /recordings/:id/download` (edge proxy fallback) — all in `sentinel-control/src/domain/recordings.rs`, gated by `recordings.read`.
- **Edge = a `tenant_boxes` row** identified by a box token (`sbx_…`, sha256 → tenant). Enrollment: pairing code (`box_pairings`, 15-min TTL) redeemed at `POST /enroll` → mints box token + Tailscale key. Health/reports piggyback on the ~25s config long-poll via `X-Edge-*` headers. Edge is pure-Rust (motion MOG2 + tract YOLOX are pure Rust); the hard mobile bits are go2rtc, ffmpeg, and the Tailscale CLI.

---

## Phase 1 — App skeleton + auth + notifications-as-Recipient (the MVP core)

The single most valuable slice: a phone that logs in and receives Sentinel alerts as a native push, indistinguishable from Telegram in the operator's mind.

**Backend — generalize the Recipient channel (additive, no Telegram regression):**
- Migration (next number — `ls crates/app-core/migrations/`, currently next is **0067**): new table `push_devices` (`tenant_id`, `user_id`, `device_token`, `platform` `ios|android`, `label`, `enabled`, `created_at`, `last_seen`; RFC3339 `text` timestamps per project rule). Keep `notification_targets` untouched; push is a parallel channel, not a column bolt-on, so Telegram code is unaffected.
- Add a `channel` discriminator to the notification `work_jobs` payload (`crates/app-core/src/work_queue.rs` `enqueue_notification_*`) — default `"telegram"` so existing enqueues are unchanged. Branch in `sentinel-notification/src/consumer.rs::process`: `telegram` → today's path; `push` → new `push` client.
- New `crates/app-core/src/notification/push.rs` client: send to FCM (Android) + APNs (iOS). Expo's push service (`https://exp.host/--/api/v2/push/send`) is the fast path for Phase 1 — one HTTP endpoint for both platforms, no APNs cert juggling — behind the same rustls/ureq style used elsewhere. Operator-level provider config (Expo access token / FCM key) alongside `telegram_global_config` (admin providers page pattern, `sentinel-control/src/domain/admin_providers.rs`).
- Generalize recipient resolution so `NotifyRecipient` can fan out to a recipient's registered channels (Telegram chat and/or push devices). Keep the `Notifier` trait Telegram-shaped for now and add a parallel push dispatch rather than a risky trait rewrite; unify later if it pays off.
- Control endpoints: `POST /push/devices` (register token, auth = user JWT), `DELETE /push/devices/:id`, `GET /push/devices`. Surface push devices in the Recipients screen so the web UI shows "this operator's phone" as a Recipient.

**App (Expo):**
- New top-level dir `sentinel-mobile/` (Expo managed workflow, TypeScript). Shared config for `control`/`gateway` base URLs mirroring `sentinel-app/src/lib/config.ts`.
- Login screen → `POST {control}/auth/login`, store JWT in `expo-secure-store`. (Google OAuth deferred to Phase 5.)
- `expo-notifications`: request permission, get Expo push token, `POST /push/devices` on login. Foreground + background handlers; tapping a notification deep-links to the relevant camera/live/recording.
- Minimal shell: a couple of tabs (Alerts, Live, Recordings) with placeholders wired in later phases.

**Done when:** a workflow `NotifyRecipient` to a recipient with a registered phone delivers a native push on iOS + Android, and Telegram delivery is byte-for-byte unchanged.

## Phase 2 — Live camera view (like the Live Tab)

- Camera list from `GET {control}/devices` (bearer). Derive stream name `cam_<ip→underscores>` client-side (mirror `streamName()` in the Live Tab; go2rtc naming lives in `crates/app-core/src/go2rtc.rs`).
- Live playback: open `wss://{gateway}/api/ws?src=<name>[_hd]&token=<jwt>`, handshake `{type:"mse", value:<codecs>}`, feed fMP4. React Native has no `MediaSource`; two viable renderers — (a) a `react-native-webview` hosting the same MSE player as the web Live Tab (fastest reuse, ship this first), or (b) `react-native-webrtc` if we later add a WebRTC path at the gateway. Start with the WebView MSE bridge to reuse proven code.
- SD/HD toggle (SD = substream default, HD = `_hd`), stall watchdog + auto-retry, honest connecting/live/stalled/error states — port the Live Tab's state machine.
- Requires `live.view` perm (Admin) — reuse the web app's gating; show a clear "ask an admin" state otherwise.

## Phase 3 — Recordings playback

- List: `GET {control}/recordings` (filters camera_ip/from/to, paginate) — mirror `sentinel-app/src/routes/recordings`.
- Play: `GET /recordings/:id/url` → presigned R2 MP4 URL (supports range) → native `expo-av` / `react-native-video` player. Fallback to `GET /recordings/:id/download` (bearer, edge proxy) when not yet in cloud (409).
- Optional download-to-device via the presigned URL.
- Requires `recordings.read`.

## Phase 4 — Ground-prep: phone as an edge (transparent camera)

_Deliberately "prepare the ground," not full parity. The phone becomes just another `tenant_boxes` row with one synthetic self-camera._

- **Enrollment**: reuse the pairing-code flow — the app accepts a pasted code / deep-link and redeems `POST /enroll` → stores box token in `expo-secure-store`. No control-side change (a phone is another box). Health/report protocol (`/config/*` POSTs, `X-Edge-*` headers) is plain HTTP/JSON — reusable as-is.
- **Native edge core via UniFFI**: compile the pure-Rust pieces (`mog2.rs` motion, tract YOLOX detector) into a mobile library; the OS camera API feeds decoded frames in. Motion/detection run on-device, reporting `motion`/`track-event`/`face` to control exactly like a Linux edge.
- **Synthetic camera**: instead of ONVIF discovery, inject one `Device` representing the phone's own camera (stable `id`, pseudo-`ip`/device-id, `rtsp_url` = the phone's local stream). Control's `configured_cameras` is IP-keyed — decide (Open decision) whether to mint a synthetic IP or extend the model.
- **The hard part — reachability + live**: the cloud live path assumes go2rtc at `:1984` with `cam_<ip>` streams over the tailnet. On the phone we must either bundle go2rtc (feasible Android, **not** iOS) or stand up a **minimal go2rtc-compatible shim** (`/api/frame.jpeg?src=cam_…` + the MSE WS) that the gateway/flow hit unchanged, and join the tailnet via the mobile Tailscale/tsnet library (no CLI). Stream naming must match `app_core::go2rtc::stream_name`.
- **Scope for this phase = a spike + a written parity matrix + a working motion-only report path** (phone enrolls, joins tailnet, reports motion; live-from-phone can trail as its own card once the shim approach is proven).

## Phase 5 — Polish (post-MVP)

- Google OAuth sign-in, biometric unlock, per-camera notification preferences, background live-preview thumbnails, OTA update channel, app-store packaging (reuse Apple signing infra from #82 for the iOS side).

---

## Open decisions
1. **Push transport for Phase 1**: Expo Push service (one endpoint, fastest) vs. talk FCM/APNs directly (more control, more setup). Recommend Expo first, revisit if we outgrow it.
2. **Recipient model**: is a "phone" its own Recipient row, or a delivery channel attached to an existing Recipient (so one Recipient can be Telegram *and* push)? Leaning: channels attached to a recipient identity, but MVP can ship phone-as-its-own-Recipient.
3. **Synthetic camera identity** (Phase 4): mint a synthetic IP for the phone camera vs. extend `configured_cameras`/`Device` to be non-IP-keyed.
4. **Live renderer** (Phase 2): WebView-hosted MSE (reuse) vs. adding a WebRTC path at the gateway for native playback. Start WebView; measure.
5. **Repo placement**: `sentinel-mobile/` in this monorepo vs. a separate repo. Leaning monorepo for shared types/config.

## Out of scope (v1)
- Full edge parity on the phone (recording ring, face embedding, ONVIF) — only motion/detection + reporting in Phase 4.
- iOS bundling of go2rtc/ffmpeg subprocesses (not possible on iOS) — the shim approach replaces it.
- Rewriting the `Notifier` trait to be fully channel-generic — parallel push path instead, unify later.
- WebRTC live path — MSE-over-WS reuse first.
