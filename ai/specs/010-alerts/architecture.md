# Architecture Notes — Alerts Registry

## Components

- `src/alerts/domain.rs` — `Alert`, `AlertLevel`, `Delivery`, `AlertStore`
  (`Arc<Mutex<Vec<Alert>>>`), `load`/`save` (`alerts.json`).
- `src/alerts/actions.rs` — `create()`, `deliver()`, `delete()` — shared by UI + tools.
- `src/alerts/handler.rs` — `AlertsScreen` (Browse/Create modes, cursor, draft fields).
- `src/alerts/view.rs` — dashboard (level/room stats), alert list, create form.
- Delivery uses `src/notification/handler.rs::dispatch`.

## APIs

None directly; delivery fans out through the notification layer (Telegram).

## Data Flow

Create (UI form or `create_alert` tool) → `actions::create` appends to the shared store and
saves `alerts.json`. Deliver → `actions::deliver` iterates enabled beacons, calls
`notification::dispatch` on a background thread, and records `Delivery` entries; status
returns over the `alert_status_tx` channel.

## Important Decisions

- Operations live in `actions.rs` so UI and agent tools share one implementation.
- RFC3339 timestamps + per-alert delivery records make notifications auditable.

## Risks

- Multi-beacon delivery is backgrounded to avoid blocking the tick loop.
