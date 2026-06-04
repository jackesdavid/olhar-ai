# Spec: Telegram Notifications (Beacons)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/notification/`.
> Documents existing behavior; no code change implied.

## Goal

The operator pairs Telegram recipients ("beacons") securely and sends them messages and
camera snapshots.

---

## User Value

Reaches people off-console the instant something matters, with visual context, so response
doesn't depend on the operator phoning around.

---

## Requirements

- Secure pairing: a recipient sends `/pair <code>` to the bot; a poller validates and stores
  the chat as a beacon.
- Beacons screen: list paired targets, enable/disable, test-send, rename.
- Send a message (optionally with a fresh camera snapshot) to one or all enabled beacons.
- Persist beacons to `notifications.json`.
- Expose sending to Bishop via the `send_notification` tool.

---

## Acceptance Criteria

- [x] A recipient can pair by sending the correct `/pair <code>` to the bot.
- [x] Paired beacons appear on the Beacons screen and persist to `notifications.json`.
- [x] Operator can enable/disable, rename, and test-send a beacon.
- [x] `send_notification` delivers text (and optional snapshot) to enabled beacons.
- [x] Wrong/missing pairing code does not create a beacon.

---

## Dependencies

- 002-camera-discovery (optional snapshot attachment)

---

## Risks

- The pairing poller runs continuously in the background; it must mutate the shared store
  safely and tolerate Telegram outages.
- Requires `TELEGRAM_BOT_TOKEN` (send) and `TELEGRAM_PAIRING_CODE` (pairing).

---

## Notes

`NotificationTarget { label, chat_id, enabled }`. Telegram Bot API: sendMessage / sendPhoto /
getUpdates. `dispatch()` is the core send fn reused by alerts (010) and Workflows (013).
