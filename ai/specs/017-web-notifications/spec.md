# Spec: 017 — Web migration: Notifications / Beacons (+ deliver_alert)

> **Status: DONE (code) — web migration unit (M1, finisher).** cargo check + npm build clean (15 core tests); ratatui src/ untouched. Pairing/sending/deliver are runtime-verified with a real bot token. Completes M1.

## Goal

The operator pairs Telegram recipients ("beacons") and delivers alerts/messages to them from
the web app.

## User Value

Closes the core surveillance loop: an alert can be pushed to people off-console — finishing M1
(devices → live → alerts → notifications).

## Requirements

- Copy `src/notification` (`domain`, `client`, `dispatch` + the pairing poller) into
  `app/crates/core` (drop the ratatui `BeaconsScreen`/view). Persistence path-parameterized →
  app-data `notifications.json`.
- **Telegram config in-app, not just env:** store `{ bot_token, pairing_code }` in
  `<app_data_dir>/telegram.json` (settable from the UI), with env fallback
  (`TELEGRAM_BOT_TOKEN`/`TELEGRAM_PAIRING_CODE`).
- **Pairing poller**: background task (started at setup if config present) polling Telegram
  `getUpdates`; on a valid `/pair <code>` it appends a beacon + persists.
- **Wire `deliver_alert`** (the piece deferred from 016): add `deliver` to `crates/core/alerts`
  (now that `notification` is in core) — send to all enabled beacons, record `deliveries`,
  persist. Add a Deliver action to the Alerts table.
- Commands: `list_beacons`, `set_telegram_config`/`get_telegram_config` (token masked),
  `test_beacon`, `toggle_beacon`, `rename_beacon`, `delete_beacon`, `deliver_alert`.
- Beacons screen: Telegram config + pairing instructions + paired-beacons list (enable/disable,
  rename, test, delete).

## Acceptance Criteria

- [ ] Telegram config persists; the pairing poller runs and adds a beacon on `/pair <code>`.
- [ ] `test_beacon` sends a Telegram message; `deliver_alert` delivers to enabled beacons and
      records deliveries (the alert shows delivered).
- [ ] Beacons screen + the Alerts "Deliver" action work; `cargo check` + `npm run build` clean;
      ratatui `src/` untouched.

## Dependencies

- 016 Alerts (deliver bridges to it), M0.

## Risks

- Pairing/sending need a real `TELEGRAM_BOT_TOKEN` + a chat to validate (runtime); the registry +
  UI are buildable here. Restarting the poller after a config change may require an app restart
  (acceptable v1).

## Notes

Completes **M1 — the core surveillance loop**.
