# Spec: Alerts Registry

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/alerts/`.
> Documents existing behavior; no code change implied.

## Goal

The operator records, reviews, and delivers alerts, each with a level, message, optional
room, source, and delivery history.

---

## User Value

A single log of what happened and who was told, so incidents are tracked and notifications
are accountable rather than fire-and-forget.

---

## Requirements

- Create alerts with level (Debug/Info/Warn/Error), message, optional room link, source.
- Dashboard showing today's alerts: counts by level, counts by room, pending count.
- A create form (modal) to raise an alert manually.
- Deliver an alert to all enabled beacons and record each delivery (target, time, ok).
- Persist to `alerts.json`; expose the same operations to Bishop's tools.

---

## Acceptance Criteria

- [x] Operator can create an alert via the form with a level and message.
- [x] The dashboard shows level/room counts and a pending count for today.
- [x] Delivering an alert sends it to enabled beacons and records the deliveries.
- [x] Alerts (and their delivery records) persist to `alerts.json`.
- [x] Browse and Create modes are both usable from the alerts screen.

---

## Dependencies

- 011-notifications (delivery targets / beacons)
- 009-floorplan-map (optional room link)

---

## Risks

- Delivery to many beacons must not block the UI — runs on a background thread.

---

## Notes

`Alert { id, level, message, room_id, room_name, source, created_at (RFC3339), deliveries[] }`;
store is `Arc<Mutex<Vec<Alert>>>`. `actions.rs` (`create`/`deliver`/`delete`) is shared by the
UI and the agent tools, and is reused by Workflows (spec 013).
