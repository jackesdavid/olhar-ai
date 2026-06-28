# Telegram Bot — Workflow-Graph Brain + Inbound Commands

**Board card:** #15 Telegram bot: workflow-graph brain + inbound commands (ToDo)
**Goal:** Make the Telegram bot's response logic a live workflow graph — when a user sends a message, the engine fires a cross-graph event, not hardcoded Rust handlers. Define which existing nodes cover which commands and which new nodes must be built.

---

## Architecture principle

The "bot brain" is a **workflow graph**. When a paired Telegram user sends a message:

1. The long-poll loop (`sentinel-flow/src/pairing.rs` → `notification/handler.rs`) receives the update.
2. If it isn't a pairing code, and the `chat_id` is a known paired target, it calls `engine.fire_event(tenant_id, "telegram_message", payload)`.
3. Every graph in that tenant that has an **`On event("telegram_message")`** entry node fires with the message payload injected as its entry `PortValues`.
4. The graph nodes do the work — routing, camera queries, AI calls, replies.

The user builds the bot brain in the workflow editor using the familiar **`On event`** node — no new trigger type, no new UI concept. They set the event name to `telegram_message` (a well-known constant, listed in the event picker).

```
On event("telegram_message") → Get Telegram message → ...graph logic...
```

---

## Existing nodes that apply — no changes needed

| Use case | Node(s) |
|---|---|
| Route `/snapshot` vs `/ask` vs `/status` | **String switch** on `command` output of `Get Telegram message` |
| Take a camera photo | **Camera snapshot** — wire `args` from `Get Telegram message` to `camera` input |
| Rate-limit spam | **Cooldown** — gate the whole graph or per-command branch |
| Conditional logic | **If**, **And**, **Or**, **Not**, **Compare**, **Text compare** |
| Describe what a camera sees (text caption) | **Describe image** — then wire `description` to **Reply to sender** |
| Detect motion / people | **Person detection**, **Motion detection**, **Get current frame** |
| Create or read alerts | **Create alert**, **Get last alert**, **Deliver alert** |
| Format a reply string | **Format** |
| Debug | **Print**, **Show image** |
| Loop over detections | **For each** + **Break person detection** |
| Entry point for the bot brain | **On event** — event name: `telegram_message` |

---

## New nodes to build

Three focused additions. The `On event` node already handles the entry point.

### 1. `Get Telegram message` — source node

Reads the event payload that `fire_event` injected into the run's entry `PortValues`. Placed right after `On event("telegram_message")`.

| | |
|---|---|
| **Inputs** | none |
| **Outputs** | `text` (Text), `chat_id` (Num), `from_name` (Text), `command` (Text — first word if starts with `/`, else empty), `args` (Text — everything after the command, or full text) |
| **Config** | none |
| **Behavior** | Pure source — reads from `ctx.entry_values`. If `command` is empty, `String switch` default arm fires naturally. |

**Where to add:**
- `domain.rs`: `NodeKind::GetTelegramMessage` + ports + `NodeKind::ALL`
- `node.rs`: `run_get_telegram_message()` — extract fields from `ctx.entry_values`

**Sample usage:**
```
On event("telegram_message") → Get Telegram message
  → command → String switch (arms: /snapshot, /ask, /status)
```

---

### 2. `Reply to sender` — action node

Sends a text or photo reply back to the `chat_id` that triggered this run. Unlike `Notify recipient`, this targets the specific chat that sent the message — not a configured outbound recipient.

| | |
|---|---|
| **Inputs** | `message*` (Text), `chat_id*` (Num), `image` (Image, optional) |
| **Outputs** | `ok` (bool), `result` (Text) |
| **Config** | `message` (literal fallback) |
| **Behavior** | Calls `notification::client::send_photo()` if image wired, else `send_message()`. Resolves bot token the same way `Notify recipient` does (tenant → global → env). |
| **Timeout** | 35 s (same class as notify/deliver) |

**Where to add:**
- `domain.rs`: `NodeKind::ReplyToSender` + ports
- `node.rs`: `run_reply_to_sender()` — thin wrapper over `notification::client`

**Sample usage:**
```
Get Telegram message → (chat_id) ──────────────────────────────┐
Get Telegram message → (args) → Camera snapshot → (image) → Reply to sender
```

---

### 3. `Ask AI` — text-in/text-out AI node

Sends a text prompt (with optional image and context) to Claude and returns a plain-text response in a single turn. Complements `Describe image` (image-only caption) and `AI If` (binary routing).

| | |
|---|---|
| **Inputs** | `prompt*` (Text), `context` (Text, optional), `image` (Image, optional) |
| **Outputs** | `response` (Text), `ok` (bool) |
| **Config** | `prompt` (literal), `timeout_secs` (default 30) |
| **Behavior** | Single-turn Claude call — no tool-use loop (too slow for Telegram). If `image` is wired, sends it as a vision message. If `context` is wired, prepends it to the user message. |

**Where to add:**
- `crates/app-core/src/agent/` — add `ask_single_turn(prompt, image_opt, context_opt, cfg) → Result<String>` (or reuse `client::ask()` with an empty tool registry)
- `domain.rs`: `NodeKind::AskAi` + ports
- `node.rs`: `run_ask_ai()` — calls the single-turn helper under timeout

**Sample usage:**
```
Get Telegram message → (args) → Ask AI(prompt="You are Sentinel. Answer: {p1}", p1=args)
  → response → Reply to sender
```

---

## Reference workflow graphs

### Graph A — Basic command bot

```
On event("telegram_message")
  ↓
[Get Telegram message]
  ├─ command ──▶ [String switch (arms: /snapshot, /ask, /status)]
  │                 ├─ /snapshot ──▶ [Camera snapshot (camera=args)] ──▶ [Reply to sender (image)]
  │                 ├─ /ask ──▶ [Ask AI (prompt=args)] ──▶ (response) ──▶ [Reply to sender]
  │                 ├─ /status ──▶ [Format "Connected ✓ — try /snapshot <cam>"] ──▶ [Reply to sender]
  │                 └─ default ──▶ [Reply to sender "Unknown command. Try /snapshot, /ask, /status"]
  └─ chat_id ─────────────────────────────────────────────────────────────▶ all Reply to sender nodes
```

---

### Graph B — AI free-text bot

No command syntax — every message goes to Claude with Sentinel context.

```
On event("telegram_message")
  ↓
[Get Telegram message]
  ├─ text ──▶ [Cooldown(30s)] ──▶ [Ask AI (prompt="You are Sentinel. {p1}", p1=text)]
  │                                    └─ response ──▶ [Reply to sender]
  └─ chat_id ──────────────────────────────────────────▶ [Reply to sender]
```

---

### Graph C — Motion alert with Telegram snooze

```
On event("telegram_message")  (Concurrency: SkipIfRunning)
  ↓
[Get Telegram message]
  └─ command → String switch
       ├─ /snap ──▶ [Camera snapshot(camera=args)] ──▶ [Reply to sender (with image)]
       └─ /snooze ──▶ [Cooldown(3600s)] ──▶ [Reply to sender "Alerts snoozed 1h"]
  └─ chat_id ──▶ all Reply to sender
```

---

## Implementation phases

### Phase 1 — Engine: fire cross-graph event from Telegram long-poll
1. `crates/app-core/src/notification/handler.rs`: after the pairing-code check fails AND `chat_id` is a known paired target → call `engine.fire_event(tenant_id, "telegram_message", payload)` where payload is `PortValues { text, chat_id, from_name }`.
2. `sentinel-flow/src/pairing.rs`: pass an engine event channel (or `Arc<Engine>`) into the pairing spawner so `fire_event` is reachable from the long-poll thread.
3. The engine's existing `fire_event()` already scans all tenant graphs for matching `OnEvent` entry nodes — no new dispatch logic needed.
4. Register `"telegram_message"` as a well-known event name in the editor's event picker (alongside any future system events).

### Phase 2 — `Get Telegram message` node
- `domain.rs` + `node.rs` (~20 lines each).
- Reads `text`, `chat_id`, `from_name` from `ctx.entry_values`; derives `command` + `args` inline.

### Phase 3 — `Reply to sender` node
- `domain.rs` + `node.rs` — calls `notification::client::send_message/send_photo` with the wired `chat_id`.
- Bot token resolved same way as `Notify recipient`.

### Phase 4 — `Ask AI` node
- `crates/app-core/src/agent/` — `ask_single_turn()` helper.
- `domain.rs` + `node.rs` under 30 s timeout.
- Log missing `ANTHROPIC_API_KEY` as warning at startup.

### Phase 5 — Palette + docs
- Category: `Get Telegram message` + `Reply to sender` → **"Telegram"**; `Ask AI` → **"Vision / AI"**.
- Update `docs/workflow-nodes.md`.

---

## Open decisions

1. **Engine handle in long-poll thread**: the pairing loop uses `std::thread`; `fire_event` lives on the async runtime. Bridge: `tokio::sync::mpsc::Sender<FireEventCmd>` passed into the pairing spawner, consumed by an async task in the engine.
2. **`Ask AI` tool-use**: single-turn keeps latency ~2-3 s (acceptable for Telegram). Full tool-use loop (15+ s) deferred to follow-up card linked to #16.
3. **Event name casing**: `"telegram_message"` (snake_case) matches the existing event naming convention in the codebase.
4. **No-args `/snapshot`**: `args` is empty → `Camera snapshot` falls back to `camera = "first"`. Acceptable v1 behavior.

---

## Out of scope (v1)

- Inline keyboard buttons / callback queries
- Multi-turn stateful conversations (each graph run is stateless per message)
- `/arm` / `/disarm` commands (need confirmation flow)
- Cross-graph routing beyond what `On event` + fan-out already provides
- `List cameras` node for rich `/status` replies
