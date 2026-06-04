# Domain Glossary

Consistent terminology improves communication, AI consistency, and implementation quality.

| Term | Meaning |
|------|---------|
| Operator | The single human at the console; the primary user. |
| Bishop | The in-app AI synthetic (Claude) the operator chats with; can use tools. |
| Spec | One incremental, testable implementation unit (one feature folder). |
| Screen | A top-level view in the app, selected via the `Screen` enum (menu, scan, chat, map, alerts, beacons, device, config, about). |
| Slice | A feature folder under `src/` following the domain/handler/view split. |
| Device | A camera in the registry (`devices.json`): ip, xaddr, name, credentials, rtsp_url. |
| ONVIF | Standard used to discover cameras and run WS-Security SOAP control calls. |
| Snapshot | A still JPEG captured from a camera over HTTP, decoded and shown as glyphs. |
| Live feed / Stream | An RTSP feed pulled via ffmpeg and rendered frame-by-frame in the terminal. |
| Tool | A capability exposed to Bishop (the AI) that acts on operator state (e.g. `create_alert`). |
| Room | A rectangle on the floor-plan map (`map.json`): id, name, x, y, w, h, cameras[]. |
| RoomCamera | A camera placed inside a room at a (dx, dy) offset from the room's top-left. |
| Alert | A logged event (`alerts.json`): level, message, optional room, source, created_at, deliveries[]. |
| AlertLevel | One of Debug / Info / Warn / Error. |
| Deliver | Send an alert to all enabled beacons and record who received it and when. |
| Beacon / Notification target | A paired Telegram chat (`notifications.json`): label, chat_id, enabled. |
| Pairing | Secure handshake where a recipient sends `/pair <code>` to the bot to become a beacon. |
| Beep / Sfx | Terminal audio effects (boot loop, keystrokes, working sound) via rodio. |
| CRT sweep | The phosphor/scanline post-processing on the optional wgpu window. |
| Activity / ActivityGuard | The shared counter that drives the "PROCESSING" top-bar indicator while background work is in flight. |
| Beacon (beacons screen) | The management screen listing paired Telegram targets. |
