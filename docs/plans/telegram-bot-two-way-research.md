# Telegram Bot — Workflow-Graph Brain + Inbound Commands

**Board card:** #15 Telegram bot: workflow-graph brain + inbound commands (In Progress)
**Depends on:** #57 (notification service + durable queues) — **DONE**. This plan is refreshed to
build on what #57 shipped.
**Goal:** Make the bot's response logic a live **workflow graph**. A paired user's message fires an
`On event("telegram_message")` trigger; the graph does the work (route, snapshot, ask AI, reply).
**Hard rule:** everything is **tenant-scoped** — only the tenant that owns a `chat_id` receives its
`telegram_message` events (naturally satisfied: one long-poll poller per tenant, and we only enqueue
for `chat_id`s already paired in *that* tenant's beacon store).

---

## What #57 changed (why this plan is much smaller now)

The old plan's hardest piece — getting an engine handle into the Telegram long-poll thread — **no
longer exists**. #57 moved pairing into `sentinel-notification` and built the durable **`event_queue`**
rail (producer → Postgres → flow's `event_consumer` → `engine::fire_event`). So inbound is now a
one-line **enqueue**, not an engine bridge.

```
INBOUND  (new work = the enqueue call + a system event)
  Telegram getUpdates ─▶ notification pairing loop (handler.rs, per tenant)
      not a pairing code? chat_id ∈ this tenant's beacons?
        └─▶ app_core::event_queue::enqueue(db, tenant, "telegram_message", "telegram", payload)
  Postgres event_queue ─▶ flow event_consumer (DEPLOYED) ─▶ fire_event
        └─▶ OnEvent("telegram_message") graphs run with payload as typed outputs

OUTBOUND (new work = 2 nodes + a send_video path)
  graph … ─▶ [Reply to sender]  (text/photo: synchronous send to the sender's chat_id)
  graph … ─▶ [Reply with recording] (video: fetch clip from edge → object store → queue → send_video)
```

**Current gap being closed:** `handler.rs:115-131` advances the update offset then, for any
non-pairing message, hits `continue` — the message is **silently dropped**. We replace that `continue`
with the enqueue.

---

## Phase 1 — Inbound: `telegram_message` as a durable event (the core)

**1a. Register `telegram_message` as a system event** so every tenant's `On event` node shows typed
outputs with no prior registration.
- `crates/app-core/src/workflows/repo.rs` → `system_events()` (`repo.rs:282-319`, next to `motion`).
- Fields (all derived at enqueue time — see 1b): `text:Text`, `chat_id:Num`, `from_name:Text`,
  `command:Text`, `args:Text`. (`command`/`args` are computed once at the source so no parse node is
  needed downstream — `String switch` wires straight to `command`.)

**1b. Enqueue instead of dropping** in the long-poll loop.
- `crates/app-core/src/notification/handler.rs` `spawn_pairing` loop (`handler.rs:108-131`). The loop
  already owns `db: Option<Db>` + `tenant_id` + `store` (the tenant's `NotificationStore`).
- New branch when `parse_pairing_code(...)` is false:
  ```
  // known sender only (tenant-scoped): chat_id must be an enabled paired target
  if store.lock().targets.iter().any(|t| t.enabled && t.chat_id == u.chat_id) {
      let (command, args) = split_command(&u.text);   // "/snap cam1" → ("/snap","cam1")
      let payload = json!({ "text": u.text, "chat_id": u.chat_id,
                            "from_name": u.sender_label, "command": command, "args": args });
      if let Some(db) = &db { app_core::event_queue::enqueue(db, tenant_id, "telegram_message", "telegram", &payload); }
  }
  ```
- `split_command`: first whitespace-delimited word is `command` **iff** it starts with `/` (else
  `command=""`, `args=text`); remainder is `args`. Small pure helper in `handler.rs` (unit-testable).
- `Update` already projects `{ update_id, chat_id, text, sender_label }` (`client.rs:101`).

**No flow changes needed for inbound** — `event_consumer::payload_to_portvalues` already maps
`{text, chat_id, command, args}` → `Text`/`Num` outputs, and a base64-JPEG string → `Image` (so a
future inbound photo rides inline). `fire_event` no-ops when a tenant has no handler graph.

**Security check:** we enqueue only for `chat_id`s in the *current poller's* tenant store, so a message
can never fan out to another tenant. Matches the hard rule.

---

## Phase 2 — `Reply to sender` node (text + photo, synchronous)

Sends back to the **specific chat that triggered the run** (not a configured beacon). Synchronous
in-process send — a bot reply is interactive, so immediate delivery + real `ok`/`result` beats queue
latency. (Heavy/lossy-sensitive alert sends still go through the #57 queue; short replies don't.)

| | |
|---|---|
| **Kind** | `NodeKind::ReplyToSender` — category **"Telegram"** (new category) |
| **Inputs** | `message*` (Text), `chat_id*` (Num), `image` (Image, optional) |
| **Outputs** | `ok` (Bool), `result` (Text) |
| **Behavior** | Resolve tenant bot token from `sys.notif_cfg` (same handle `NotifyRecipient` uses, `recipients.rs:23`). If `image` wired → `notification::client::send_photo(cfg, chat_id, message, &jpeg)`; else `send_message(cfg, chat_id, message)`. Return `ok`+`result`. |
| **Timeout** | 35 s (notify class). |

- `domain.rs`: variant + ports + category arm + slug/label + add to `ALL` (bump size).
- `node.rs`: dispatch arm → `node/telegram.rs::run_reply_to_sender` (new submodule); calls
  `crate::notification::client` directly (same crate) with `sys.notif_cfg`.
- No new client code, no schema change.

---

## Phase 3 — `AI Answer` node (text-in → text-out)

Single-turn Claude: prompt in, plain text out. **Note the collision:** `NodeKind::AskAi` already
exists — it's the yes/no "AI Branch" node (`ai.rs:25`, outputs `Bool`). So this is a **new** kind.

| | |
|---|---|
| **Kind** | `NodeKind::AiAnswer` — label "AI Answer", category **"AI"** |
| **Inputs** | `prompt*` (Text), `image` (Image, optional) |
| **Outputs** | `response` (Text), `ok` (Bool) |
| **Config** | `prompt` (literal fallback) |
| **Behavior** | New `Vision::ask_text(prompt, image: Option<&[u8]>) -> Result<String>` on the trait (`ports.rs:38`), `LocalVision` impl delegating to a new `agent::vision::ask_text` that mirrors `describe_image`'s single POST (`vision.rs:25`) but takes a free-text prompt (higher `max_tokens`, image optional). Auth via `AgentConfig::from_env` (`ANTHROPIC_API_KEY` + `ANTHROPIC_MODEL`, default `claude-opus-4-8`). |
| **Timeout** | 30 s. No tool-use loop (that's ~15 s+; single turn is ~2-3 s). |

- `node.rs` dispatch arm → `ai::run_ai_answer` (mirror `run_describe_image`, `ai.rs:5`).
- Startup warn if `ANTHROPIC_API_KEY` unset (offline → node returns `ok:false`, `response:""`).

---

## Phase 4 — Video reply (finish the #57 half-built path)

Reply to the sender with a **recording clip**. #57 already built the media rails (`media::put/get`,
`notification_queue.video_key`, and a consumer stub that logs *"video send lands with #15"*
`consumer.rs:49-51`). Remaining work, all small:

1. **Edge clip fetch** — new GET on the edge + a `recording.rs` helper: `GET {edge_api}/recording/{clip_id}`
   → mp4 bytes (`resolve_edge_api_base`, `recording.rs:92`; only start/stop exist today).
2. **`send_video`** — add to `notification/client.rs` (multipart `sendVideo`, mirror `send_photo`
   `client.rs:34`); add a video arm to `dispatch` (`handler.rs:64`).
3. **Raw chat_id in the outbound queue** — the queue targets only beacon labels today. Migration
   **0036**: `ALTER TABLE notification_queue ADD COLUMN chat_id bigint NULL`; consumer sends direct to
   `chat_id` when set (else beacon path). Thread `chat_id` through `queue::enqueue`.
4. **`Reply with recording` node** (`NodeKind::ReplyWithRecording`, "Telegram"): inputs
   `clip_id*` (Text, from `Start recording`'s `clip_id` output `domain.rs:646`), `chat_id*` (Num),
   `caption` (Text). Fetches clip → `media::put(bytes,"video/mp4","mp4")` → `queue::enqueue(chat_id,
   video_key)`. Async by design (video is heavy) — outputs `queued:Bool`.

*Phase 4 is the one slice that touches the edge + a migration; Phases 1-3 ship the interactive
text/photo bot on their own. Ship 1-3 first, then 4.*

---

## Phase 5 — Palette, invariants, docs

- **Node count**: `NodeKind::ALL: [NodeKind; 56]` (`domain.rs:685`) → bump to `59` (Phases 2-4 add
  ReplyToSender, AiAnswer, ReplyWithRecording). Update the `56` in `CLAUDE.md:64`.
- **Category**: add `"Telegram"` arm in `NodeKind::category` (`domain.rs:874-927`).
- `slug()`/`label()`/`from_slug` round-trip covered by `slug_round_trips_all_kinds` (`domain.rs:1921`).
- Update `docs/workflow-nodes.md` (node catalog) + a "Bot brain" recipe section.
- Agent chat tools (`agent/tools.rs`) need **no** change — it doesn't list workflow nodes (per CLAUDE.md).

---

## Reference graphs (unchanged in spirit; `command` now comes straight off `On event`)

**A — Command bot**
```
On event("telegram_message")
  ├ command ─▶ String switch(/snapshot, /ask, /status)
  │   ├ /snapshot ▶ Camera snapshot(camera=args) ▶ Reply to sender(image, chat_id)
  │   ├ /ask      ▶ AI Answer(prompt=args)        ▶ Reply to sender(response, chat_id)
  │   ├ /status   ▶ Format "Connected ✓"          ▶ Reply to sender(chat_id)
  │   └ default   ▶ Reply to sender("Try /snapshot, /ask, /status", chat_id)
  └ chat_id ─▶ (wired into every Reply to sender)
```
**B — Free-text AI bot**: `On event → Cooldown(30s) → AI Answer(prompt=text) → Reply to sender`.
**C — Snapshot + `/clip` video**: `String switch → /clip ▶ Start recording … Stop recording → Reply with recording(clip_id, chat_id)`.

---

## Open decisions (resolve before/while building)

1. **Reply transport** — *recommend* synchronous send for text/photo (interactive UX, real `ok`),
   queued for video (heavy). Alternative: everything through the queue (uniform, but replies get poll
   latency + no real `ok`). **Leaning: sync text/photo, queued video.**
2. **Enqueue scope** — enqueue *every* message from a paired chat (simple; `fire_event` no-ops with no
   handler) vs. only when a `telegram_message` handler graph exists (needs a listener-count check).
   **Leaning: enqueue all paired-chat messages; revisit if event_queue gets noisy.** (GC already
   retires processed rows.)
3. **Ship boundary** — Phases 1-3 (text/photo bot) as the first shippable slice, Phase 4 (video) as a
   second commit on the same card. **Leaning: yes, split.**
4. **AI backend** — Anthropic direct (`ANTHROPIC_API_KEY`) for `AI Answer`, not the RunPod vision
   sidecar (`AI_SERVICE_URL`, which is image-only). Confirm the key is set in the flow pod's env.

---

## Out of scope (v1)

- Inline keyboards / callback queries; multi-turn stateful conversations (each run is stateless).
- `/arm` `/disarm` (need a confirm flow); a `List cameras` node for rich `/status`.
- Full tool-use agentic loop in `AI Answer` (single-turn only; agentic reply is a follow-up linked to #16).
- Inbound **photos** from users (payload already supports an inline base64 image, but no node consumes it yet).
