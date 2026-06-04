# Architecture Notes — Device Detail & Camera Controls

## Components

- `src/device/domain.rs` — `DeviceDetail` state (ip, field focus, form fields, status,
  snapshot image).
- `src/device/actions.rs` — snapshot capture, ONVIF credential validation, image decode.
- `src/device/handler.rs` — form focus/navigation, snapshot/credential/rename dispatch.
- `src/device/view.rs` — form + snapshot rendering.
- `src/ui/image.rs` — `CameraImage` wrapper, Braille/quadrant downscaling, dithering.
- `src/onvif/client.rs` — credential probe; `src/scan/` device store for rename persistence.

## APIs

- HTTP GET to camera snapshot endpoint; ONVIF SOAP for credential test.

## Data Flow

Open from scan/menu with a selected ip → form populated from the device record. Snapshot and
credential actions run on background threads, posting results over the `scan_tx` channel and
holding an `ActivityGuard` so the top bar shows PROCESSING. Rename writes back to the shared
device store and `devices.json`.

## Important Decisions

- Image is downscaled to Braille/quadrant glyphs (`ui/image.rs`) for terminal display.
- Actions are isolated in `actions.rs` so both the UI handler and (later) tools can reuse.

## Risks

- Vendor snapshot-URL variance; mitigated by ONVIF-derived endpoints + tolerant decode.
