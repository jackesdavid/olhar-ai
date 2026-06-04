# Spec: Bishop's Tools (Agent Tool Surface)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/agent/tools.rs`
> and `src/agent/vision.rs`. Documents existing behavior; no code change implied.

## Goal

Bishop (the AI) can act on the operator's own state by calling a fixed registry of tools
during a conversation.

---

## User Value

The operator can delegate real actions conversationally — "show me the front door", "alert
the team" — instead of navigating screens manually.

---

## Requirements

- Expose a tool registry to the Claude API with JSON input schemas.
- Tools operate over shared state via a `ToolContext` (device store, ui channel,
  notification targets, alert store).
- Cover devices (list / snapshot / describe / live feed / delete / rename), notifications
  (send), and alerts (create / list / stats / deliver / delete).
- `describe_camera` runs a vision API call on a fresh snapshot.
- Tool side effects that touch the UI (show image, open live feed) flow over a `ui_tx`
  channel, not direct UI mutation.

---

## Acceptance Criteria

- [x] Bishop can list the operator's saved devices via `list_devices`.
- [x] `get_camera_snapshot` / `describe_camera` capture and (for describe) narrate an image.
- [x] `show_live_feed` opens the live stream modal over the current screen.
- [x] `create_alert` / `list_alerts` / `alert_stats` / `deliver_alert` / `delete_alert` act
      on the shared alert store.
- [x] `send_notification` delivers a message (optionally with a snapshot) to beacons.

---

## Dependencies

- 005-bishop-chat (the conversation that invokes tools)
- 002-camera-discovery, 004-live-stream, 010-alerts, 011-notifications (the state tools act on)

---

## Risks

- A tool that blocks would stall the agent thread — tools run inside the agent's background
  thread, never the UI thread.
- Tool side-effect modals can appear over any screen (accepted trade-off).

---

## Notes

Tool registry: `ToolSpec { name, input_schema, run }`; `ToolContext { store, ui_tx, notif,
alerts }`. This registry is the reuse foundation for the Workflows feature (spec 013).
