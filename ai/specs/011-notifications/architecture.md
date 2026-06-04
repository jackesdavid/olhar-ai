# Architecture Notes — Telegram Notifications

## Components

- `src/notification/domain.rs` — `NotificationTarget`, `NotificationConfig` (`from_env`),
  load/save (`notifications.json`).
- `src/notification/client.rs` — Telegram Bot API (sendMessage, sendPhoto, getUpdates).
- `src/notification/handler.rs` — pairing poller (background thread), `dispatch()` core send,
  `BeaconsScreen` state.
- `src/notification/view.rs` — beacons management screen.

## APIs

- Telegram Bot API (token from `TELEGRAM_BOT_TOKEN`, pairing code from
  `TELEGRAM_PAIRING_CODE`).

## Data Flow

Pairing poller thread long-polls `getUpdates`; on a valid `/pair <code>` it appends a
`NotificationTarget` to the shared `Arc<Mutex<…>>` store and saves `notifications.json`.
Sending: `dispatch(message, optional snapshot, targets)` posts to Telegram on a background
thread; results return over the `beacon_tx`/alert-status channels.

## Important Decisions

- Pairing-by-secret over open registration — only intended recipients become beacons.
- A single `dispatch()` is the one send path reused by alerts, tools, and workflows.

## Risks

- Long-running poller must handle Telegram outages and mutate the store under its mutex.
