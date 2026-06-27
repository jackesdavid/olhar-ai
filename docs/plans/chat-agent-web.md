# Chat Agent on the Web — Implementation Plan

**Goal:** Bring the conversational AI assistant (Claude + agent tools + ElevenLabs TTS/STT) to
the web app — replacing the placeholder stub with a real chat page that speaks, listens, and
acts on the user's cameras, alerts, and people.

---

## What already exists (do not rebuild)

| Layer | Location | What it does |
|---|---|---|
| Agent client | `crates/app-core/src/agent/client.rs` | `ask()` — full tool-use loop, up to 6 iters, returns `AgentOutcome` |
| Agent domain | `crates/app-core/src/agent/domain.rs` | `Message`, `Role`, `AgentConfig`, `CHAT_INSTRUCTION`, `VOICE_INSTRUCTION` |
| Tool registry | `crates/app-core/src/agent/tools.rs` | 15 tools: `list_devices`, `get_camera_snapshot`, `describe_camera`, `count_people`, `show_live_feed`, `delete_device`, `rename_device`, `list_recipients`, `send_notification`, `create_alert`, `list_alerts`, `alert_stats`, `deliver_alert`, `delete_alert`; `ToolUiEvent` variants |
| TTS | `crates/app-core/src/voice/client.rs` | `synthesize(text) → Option<Vec<u8>>` (MP3 bytes, ElevenLabs) |
| STT | `crates/app-core/src/listen/client.rs` | `transcribe(audio, filename, mime) → Result<String>` (ElevenLabs Scribe) |
| Chat stub | `sentinel-app/src/routes/chat/+page.svelte` | Placeholder — replace in place |

---

## Phase 1 — Backend: expose agent/TTS/STT routes in sentinel-control

New file: `sentinel-control/src/domain/agent.rs`

### Routes

```
POST /agent/chat   — run a conversation turn (tool loop)
POST /agent/tts    — synthesize text → MP3 bytes
POST /agent/stt    — transcribe audio upload → text
```

All three are JWT-guarded (`auth::require`) and tenant-scoped (`domain::tenant_of(&claims)`).

### `POST /agent/chat`

Request body:
```json
{ "history": [{"role": "user"|"assistant", "content": "…"}, …] }
```

Handler:
1. Build `ToolContext { db, tenant_id, … }` — `devices`, `alerts`, `notifications` come from `state.db`; the boundary ports (`FrameSource`, `Vision`, `Recognizer`, `Notifier`) need special handling (see **Open decisions**).
2. Call `app_core::agent::client::ask(&history, &registry, &ctx, false /* voice=false */)`.
3. Return `AgentOutcome { text, tool_events }` as JSON.

Response:
```json
{
  "text": "Here are your cameras…",
  "tool_events": [{"type": "Snapshot", "device_id": "…", "url": "…"}, …]
}
```

### `POST /agent/tts`

Request: `{ "text": "…" }`
Handler: calls `app_core::voice::synthesize(text)`, returns raw MP3 bytes with
`Content-Type: audio/mpeg`. Returns 503 if ElevenLabs key is missing.

### `POST /agent/stt`

Request: `multipart/form-data` with field `audio` (WebM/Opus or WAV blob from browser
`MediaRecorder`).
Handler: calls `app_core::listen::transcribe(bytes, "audio.webm", "audio/webm")`, returns
`{ "text": "…" }`.

### AppState additions

Add to `AppState` in `sentinel-control/src/main.rs`:
```rust
pub agent: Option<app_core::agent::domain::AgentConfig>,
pub voice: Option<app_core::voice::domain::VoiceConfig>,
pub stt:   Option<app_core::listen::domain::SttConfig>,
```
All read from env at boot (`AgentConfig::from_env()`, `VoiceConfig::from_env()`,
`SttConfig::from_env()`). Log enabled/disabled at startup, same pattern as Google/Tailscale.

Wire into `domain::router()` in `sentinel-control/src/domain/mod.rs`:
```rust
.merge(agent::router())
```

---

## Phase 2 — Frontend: real chat page

File: `sentinel-app/src/routes/chat/+page.svelte`

### Layout (Emerald Guardian theme, emerald `--primary`)

```
┌─────────────────────────────────────────┐
│  Assistant                              │
│  Ask Sentinel about your cameras…       │
├─────────────────────────────────────────┤
│                                         │
│  [message list — scrollable]            │
│    user bubble (right, muted bg)        │
│    assistant bubble (left, card)        │
│    tool-event panels inline (snapshot,  │
│    alert badge, live-feed link)         │
│                                         │
├─────────────────────────────────────────┤
│  [🎤 mic]  [input text field]  [Send]   │
└─────────────────────────────────────────┘
```

### State (Svelte 5 runes)

```ts
let messages = $state<{role, content, toolEvents?}[]>([])
let input    = $state('')
let busy     = $state(false)
let recording = $state(false)
```

Conversation history is kept client-side — each `POST /agent/chat` sends the full
`messages` array (stateless server).

### Send flow

1. User types → hits Send or Enter.
2. Append `{ role: 'user', content: input }` to `messages`.
3. `busy = true`; POST `/api/agent/chat` with full history.
4. Append assistant response to `messages`; render `toolEvents` inline.
5. Auto-TTS: POST `/api/agent/tts` with assistant text → receive MP3 blob → play via
   `new Audio(URL.createObjectURL(blob))`.
6. `busy = false`.

### Voice-input flow (mic button)

1. `record = true`; `MediaRecorder` starts on `getUserMedia({ audio: true })`.
2. User taps mic again (or silence) → stop recording.
3. POST `/api/agent/stt` (multipart, WebM/Opus blob).
4. Fill `input` with returned text. User reviews → presses Send.

### Tool event rendering (inline in assistant bubble)

| ToolUiEvent | Rendered as |
|---|---|
| `Snapshot` | Thumbnail `<img>` with device name caption |
| `ShowImage` | Larger `<img>` (same as Show Image node) |
| `OpenLive` | Link button → `/live?cam=…` |
| `DevicesChanged` | Inline notice: "Camera list updated" |
| `AlertsChanged` | Inline notice: "Alerts updated" |
| `Print` | Text paragraph in a `<pre>` block |
| `Presence` | Summary badge |

### Nav: unhide the chat route

In `sentinel-app/src/routes/+layout.svelte` the chat route is in the `CLOUD_HIDE` array.
Remove it so it appears in the sidebar.

---

## Phase 3 — Smoke test + deploy

1. `task cargo` — check the workspace compiles cleanly.
2. `task dev` + `task control` — open `/chat`, send "list my cameras", verify response + TTS plays.
3. Test mic: record a voice query → STT fills input → send → response.
4. `task cloud-deploy` — deploy control + web.

---

## Open decisions (resolve before Phase 1)

1. **Boundary ports for camera tools** — `ToolContext` has `FrameSource`, `Vision`, `Recognizer`,
   `Notifier` fields (for `get_camera_snapshot`, `count_people`, `describe_camera`). In the
   desktop app these were injected locally; in the cloud they need to reach the edge via gateway.
   **Options:**
   - **Stub** the boundary ports as `None` for cloud context — tools that need them return a
     "not available in cloud chat" message. Simpler; can be wired later.
   - **Proxy via gateway** — `FrameSource` GETs a snapshot from
     `sentinel-gateway/snapshot/<cam>`, returns JPEG bytes. Needs gateway URL in AppState.
   - **Recommendation:** stub for v1, wire gateway snapshot in a follow-up (see
     `sentinel-gateway/` which already serves snapshots).

2. **Conversation history: stateless vs. session** — sending full history per request is
   simple but unbounded. A server-side session (keyed by session token) would let the server
   truncate old context. For v1 stateless is fine; a 20-turn cap on the client is sufficient.

3. **TTS auto-play browser policy** — browsers block auto-play of audio unless triggered by
   a user gesture. The Send button click IS a user gesture, so starting TTS from the response
   handler should work. Test on Safari (stricter).

4. **`ANTHROPIC_API_KEY` on control** — the agent key is already read by `AgentConfig::from_env()`
   but `sentinel-control` does not currently use it. It only needs to be set in the control
   container's env (already set on VPS for the flow pod; check `deploy/app/Dockerfile` +
   the compose `environment:` section).

---

## Out of scope (v1)

- Camera snapshot / person-count tools wired to live edge via gateway (see Open decision 1)
- Persistent conversation history (sessions, Postgres)
- Streaming token-by-token response (SSE)
- Mobile push TTS
- Per-user voice selection
