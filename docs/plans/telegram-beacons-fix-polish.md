# Telegram Beacons — Fix Enrollment, Polish UX & Observability

**Use case:** (internal — no dedicated use-case file)
**Board card:** Telegram Beacons: fix enrollment + polish + observability (ToDo)
**Goal:** Make Telegram recipient enrollment work reliably, give users a one-click pairing link, and expose enough observability to diagnose delivery failures.

---

## Context — what exists

- `crates/app-core/src/notification/` — core domain: `domain.rs` (structs), `handler.rs` (pairing poller + dispatch), `client.rs` (Telegram HTTP: sendMessage, sendPhoto, getUpdates)
- `sentinel-control/src/domain/beacons.rs` — CRUD routes: `GET/PUT /telegram`, `GET/POST/PATCH/DELETE /beacons/:chat_id`
- `sentinel-flow/src/engine.rs` — spawns the pairing poller background thread per-tenant
- `sentinel-app/src/routes/beacons/` — UI page (bot token input, pairing code display, beacon list)
- DB: `telegram_config` (per-tenant bot_token + pairing_code), `notification_targets` (per-tenant chat_id + label + enabled)

### Known gaps / likely enrollment bug

The pairing poller is spawned in sentinel-flow at boot. If a tenant saves a new bot token **after** flow boots, the poller may never (re-)start for that tenant — the poller is likely only initialised once from config on startup, not on-demand when PUT /telegram fires.

---

## Phase 1 — Diagnose + fix enrollment regression

1. **Read** `sentinel-flow/src/engine.rs` — find where `spawn_pairing()` (or equivalent) is called. Confirm whether it's:
   - Called once at startup per tenant in DB, OR
   - Re-called when `telegram_config` changes (i.e., is there a config-reload path?)
2. **Read** `crates/app-core/src/notification/handler.rs` — understand `spawn_pairing` signature + how it tracks the running poller (handle stored anywhere?).
3. **Fix**: when `PUT /telegram` succeeds in `sentinel-control`, emit a cross-service signal (or, simpler, have `sentinel-flow` poll `telegram_config` for changes on a short interval and (re-)spawn the poller when the token changes). The simplest fix that works: sentinel-flow re-reads `telegram_config` on a 60-second tick and restarts the poller for any tenant whose token changed.
4. **Also verify**: the pairing code match in `handler.rs` — does it handle `/start <CODE>` (Telegram deep-link payload) in addition to `/pair <CODE>` and bare code? If not, add it here.
5. Manual test: configure bot token in UI → send `/pair <code>` to bot → beacon appears in list.

---

## Phase 2 — One-click pairing deep link

Users currently read the pairing code and type `/pair <code>` manually. We can give them a clickable link that opens Telegram and sends the code automatically.

**How it works:** `https://t.me/<BOT_USERNAME>?start=<PAIRING_CODE>` — Telegram opens the bot chat and sends `/start <PAIRING_CODE>` on the user's behalf.

### Steps

1. **Migration `0023_telegram_bot_username.sql`**: add `bot_username text` column to `telegram_config` (nullable).
2. **`client.rs`**: add `get_me(cfg) -> Result<String>` — calls `GET /bot{token}/getMe`, parses `result.username`. Call this when `PUT /telegram` saves a new token; store the returned username.
3. **`beacons.rs`**: `GET /telegram` response adds `bot_username: Option<String>`.
4. **`sentinel-app/src/routes/beacons/+page.svelte`**: if `bot_username` is set, render a "Pair now →" button as an `<a href="https://t.me/{bot_username}?start={pairing_code}" target="_blank">` — tapping it on mobile opens Telegram directly.
5. Pairing code must be ≤ 64 alphanumeric chars (Telegram limit) — validate/truncate on save if needed.
6. `handler.rs`: ensure `/start <CODE>` payloads are matched the same as `/pair <CODE>`.

---

## Phase 3 — Observability

The current UI gives no feedback about pairing state or delivery status. Add just enough to diagnose failures:

### Poller health indicator
- `handler.rs`: expose a `PairingStatus` enum: `Idle` (no config), `Polling` (running ok), `Error(String)` (last error).
- Store it in a shared `Arc<Mutex<PairingStatus>>` updated by the poller loop.
- New route `GET /telegram/status` in `beacons.rs`: returns `{ polling: bool, last_error: Option<String>, last_pair_at: Option<String> }`.
- UI: small status chip next to the bot token card — "Listening for pairs…" (green) / "Error: invalid token" (red).

### Delivery log
- Add a `beacon_deliveries` view or query in `beacons.rs`: `GET /beacons/log` returns the last 50 delivery records from the `alerts.deliveries` JSONB column (join with `alerts` table).
- UI: collapsible "Recent deliveries" section on the Beacons page — timestamp, beacon label, alert title, ✓/✗.

### Test-send button
- New route `POST /beacons/:chat_id/test` — sends a fixed test message: *"Sentinel test — delivery confirmed ✓"*.
- UI: "Test" button per beacon row.

---

## Out of scope (v1)

- Telegram webhooks (`setWebhook`) — long-poll is fine for single-VPS deployment
- Inline keyboard buttons / interactive replies
- Group/channel support
- Rate-limit backoff (low risk for current usage)
- Inbound commands (arm/disarm etc.) — that's the research card
