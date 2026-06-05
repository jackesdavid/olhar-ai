# Spec: 016 — Web migration: Alerts

> **Status: DONE (code) — web migration unit (M1).** cargo check + npm build clean (3 core tests); ratatui src/ untouched. Fully validatable here. Delivery to beacons = 017.

## Goal

The operator records, reviews, and triages alerts in the web app.

## User Value

The incident log — what happened, when, where, at what severity — feeding notifications.

## Requirements

- Copy `src/alerts` domain + the create/delete/stats logic into `app/crates/core` (drop the
  ratatui `AlertsScreen`/handler; persistence path-parameterized → app-data `alerts.json`).
- **Defer `deliver`** (needs the notification layer → unit 017). Keep the `deliveries` field on
  `Alert` (data only). `deliver_alert` lands in 017.
- Commands: `list_alerts`, `create_alert`, `delete_alert`, `alert_stats` (counts by level /
  room / today / undelivered).
- SvelteKit dashboard: stat cards (by level, today's count), an alerts table (level badge,
  message, room, source, time), and a create form (level, message, optional room, source).

## Acceptance Criteria

- [ ] `create/list/delete_alert` round-trip the registry (app-data `alerts.json`), shown in the UI.
- [ ] `alert_stats` returns counts by level + today; the dashboard renders them.
- [ ] Create form raises an alert (level/message/room/source) that appears in the list.
- [ ] `cargo check` + `npm run build` clean; ratatui `src/` untouched.

## Dependencies

- M0 foundation. (Delivery bridges to 017 Notifications.)

## Risks

- Room linkage (room_id/room_name) — the map isn't migrated yet (018); treat `room` as an
  optional free-text field for now.

## Notes

Fully buildable + validatable here (no cameras/network). Delivery to beacons = 017.
