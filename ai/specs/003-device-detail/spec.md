# Spec: Device Detail & Camera Controls

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/device/`.
> Documents existing behavior; no code change implied.

## Goal

The operator opens a single camera to inspect it: edit its details, capture a snapshot, test
credentials, and launch its live feed.

---

## User Value

Gives the operator a focused control panel per camera, so verifying and configuring one
device doesn't require leaving for other screens.

---

## Requirements

- Show a form for one camera (name, username, password) with field focus/navigation.
- Capture a still JPEG snapshot over HTTP and display it as terminal glyphs.
- Test ONVIF credentials and show a status line result.
- Rename the device; changes persist to the device registry.
- Launch the live RTSP stream for the camera.

---

## Acceptance Criteria

- [x] Operator can focus and edit name/username/password fields.
- [x] Capturing a snapshot displays the decoded image and updates the status line.
- [x] Testing credentials reports success or failure.
- [x] Renaming the device updates `devices.json`.
- [x] Launching the live feed opens the stream view for that camera.

---

## Dependencies

- 002-camera-discovery (the device must exist in the registry)

---

## Risks

- Snapshot capture blocks if done on the UI thread — mitigated by background capture.
- Camera snapshot URL/format varies by vendor (handled via ONVIF + image decode).

---

## Notes

Out of scope: PTZ control, multi-snapshot history. Snapshot description (vision) is provided
by the agent tools spec (006).
