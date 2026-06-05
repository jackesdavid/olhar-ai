# Spec: 019 — Web migration: Chat / Bishop (agent + tools)

> **Status: DONE (code) — web migration unit (M2).** cargo check + npm build clean; ratatui src/
> untouched. The agent module (`domain`/`client`/`vision`/`tools`) is copied into `app/crates/core`
> with the ratatui UI-effect channel re-pointed at Tauri events. Tool-calling + vision are
> runtime-verified by the operator (needs `ANTHROPIC_API_KEY`).

## Goal

The operator talks to Bishop in a chat screen, and Bishop acts on the console — lists cameras,
captures/looks at a camera, opens a live feed, renames/removes devices, raises/lists/delivers
alerts, and notifies a beacon — all through tool use over the already-migrated commands.

## User Value

One conversational surface that ties every migrated module together: ask in English, Bishop does
it. The headline AI feature, now on the web stack.

## Requirements

- Copy `src/agent` (`domain`, `client`, `vision`, `tools`, `mod`) into `app/crates/core/src/agent`.
  Keep the Anthropic Messages tool-use loop and one-shot vision verbatim. Persistence stays
  path-parameterized; the agent reads `ANTHROPIC_API_KEY` / `ANTHROPIC_MODEL` from env (offline
  fallback when absent), matching ratatui.
- **Re-point the UI-effect channel**, don't drop it: the ratatui `ToolUiEvent` (a `GrayImage`
  modal, styled device block, "persisted" pokes, StartLive) becomes web-friendly events the Tauri
  shell drains off the tool channel and re-emits as Tauri events while the turn runs:
  - `Snapshot { camera, jpeg }` → `chat-image` (base64 data URL, shown inline in the chat).
  - `DevicesChanged` → `devices-changed`; `AlertsChanged` → `alerts-changed` (pages live-refresh).
  - `OpenLive { ip }` → `chat-open-live` (chat offers a button → `/live?cam=<ip>`).
- **Text chat, not voice:** the system prompt is personality + a brief text-reply instruction +
  the tool-etiquette hint — NOT the spoken-interface (`VOICE_INSTRUCTION`) leg.
- **Conversation state in the backend:** a managed `Arc<Mutex<Vec<Message>>>` holds the turn
  history (with the verbatim `tool_use`/`tool_result` blocks). Commands: `send_chat(text) ->
  reply` (runs the whole tool loop on a blocking task), `reset_chat()`, `chat_ready() -> bool`
  (is an API key configured?).
- **Tool context** is built in the shell from the managed stores + app-data paths
  (`devices.json`/`alerts.json`/`map.json`/`notifications.json`) + the resolved Telegram config
  (saved `telegram.json` else env), so the tools persist and notify exactly like the typed commands.
- Chat screen: message list (user / Bishop / inline snapshots), composer, "thinking" state, an
  offline notice when `chat_ready()` is false, a reset action, and an "open live" affordance.

## Acceptance Criteria

- [ ] With `ANTHROPIC_API_KEY` set, a plain question gets a Bishop reply; "list my cameras",
      "what's on the front camera" (snapshot shown + described), and "open the front camera"
      (offers Live) all work end-to-end.
- [ ] A tool that mutates state (rename/delete device, create/deliver/delete alert, notify) takes
      effect and the matching page live-refreshes via the emitted event.
- [ ] No API key → `chat_ready()` is false and the screen shows the offline notice (no crash).
- [ ] `cargo check` + `npm run build` clean; ratatui `src/` untouched.

## Dependencies

- 014 Devices, 015 Live, 016 Alerts, 017 Notifications, 018 Map (room resolution for `create_alert`).

## Risks

- Tool use needs a real `ANTHROPIC_API_KEY` to validate (runtime); the registry + screen build here.
- Snapshot/vision hit the camera + the API synchronously — run the whole turn on a blocking task so
  the UI never stalls; tool side-effects stream to the UI via the drained channel.

## Notes

The ratatui voice/TTS path is **out of scope** here (that's M3 / 021). This unit is the text agent.
