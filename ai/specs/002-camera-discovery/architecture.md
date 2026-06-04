# Architecture Notes — Camera Discovery & Device Registry

## Components

- `src/scan/domain.rs` — `Tracker` (sweep angle), `Device` model, `ScanState`
  (devices `Arc<Mutex<Vec<Device>>>`, selected index, scanning flag).
- `src/scan/handler.rs` — selection, discovery trigger, credential test, device CRUD.
- `src/scan/view.rs` — sonar render (rotating sweep) + device list panel.
- `src/onvif/discovery.rs` — mDNS/WS-Discovery broadcast and device parsing.
- `src/onvif/xml.rs` — XML parsing (device info, service endpoints).
- `src/onvif/client.rs` — WS-Security SOAP calls (digest via `sha1` + `base64`).
- `src/onvif/domain.rs` — `DiscoveredDevice`.

## APIs

- ONVIF (SOAP/XML over HTTP) for discovery and capability/credential probes.

## Data Flow

Operator starts scan → background thread runs ONVIF discovery → results posted over the
`scan_tx`/`rx` channel → merged into the shared `Arc<Mutex<Vec<Device>>>` → list re-renders.
Save/rename/delete mutate the store and persist to `devices.json`.

## Important Decisions

- Device store is a shared `Arc<Mutex<…>>` so the agent tools can read/modify the same list.
- Flat JSON persistence (`devices.json`) over a database — local-first, single operator.

## Risks

- Discovery is environment-dependent (multicast); plaintext credential storage on disk.
