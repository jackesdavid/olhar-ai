# Spec: Camera Discovery & Device Registry

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/scan/` and
> `src/onvif/`. Documents existing behavior; no code change implied.

## Goal

The operator discovers ONVIF cameras on the local network and saves them to a persistent
device registry.

---

## User Value

Turns an unknown LAN into a named, reusable list of cameras the operator (and Bishop) can
act on, so set-up happens once instead of every session.

---

## Requirements

- Run an ONVIF discovery scan and list found cameras (ip, xaddr, manufacturer, model).
- Render a live rotating sonar sweep while scanning (Motion Tracker presentation).
- Save / edit / delete / rename devices, persisted to `devices.json`.
- Validate camera credentials via ONVIF WS-Security SOAP.

---

## Acceptance Criteria

- [x] Operator can start a scan and see discovered devices appear in the list.
- [x] Saved devices persist to `devices.json` and reload on next launch.
- [x] Operator can delete and rename a saved device.
- [x] Credential validation reports success/failure for a device.
- [x] The sonar sweep animates continuously while the scan screen is active.

---

## Dependencies

none

---

## Risks

- ONVIF discovery depends on multicast/mDNS reachability on the LAN (environmental).
- Credentials are stored in `devices.json` in plaintext (local-first trade-off).

---

## Notes

Out of scope: cloud device sync, non-ONVIF camera protocols. The shared device store is an
`Arc<Mutex<Vec<Device>>>` consumed by both this screen and the agent tools.
