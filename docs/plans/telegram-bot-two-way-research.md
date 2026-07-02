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

## Status — BUILT (commit 844fd32, 2026-07-02)

Phases 1–6 implemented (56→59 nodes), full workspace compiles, migration 0036 + the queue
`chat_id` claim path verified against a live DB. **Two things remain before the video path works
end-to-end** (text/photo bot is complete):
1. **Edge endpoint** `GET /recording/{clip_id}` → mp4 (in `sentinel-edge`, a separate crate/build).
   `recording::fetch_clip` calls it; until the edge serves it, `Reply with recording` errors cleanly.
2. **`ANTHROPIC_API_KEY`** must be set in the flow pod for `AI Answer`.
Not yet deployed.

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

OUTBOUND (everything queued — new work = chat_id in the queue + 3 nodes + send_video)
  graph … ─▶ [Reply to sender]      ─▶ notification_queue{chat_id, message, image_key?}
  graph … ─▶ [Reply with recording] ─▶ notification_queue{chat_id, video_key}
  notification consumer ─▶ send_message / send_photo / send_video  (direct to chat_id)
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

## Phase 2 — Outbound plumbing: `chat_id` in the queue + a unified consumer send

**Decision (locked): everything queued.** All replies route through the #57 `notification_queue` —
no synchronous sends. This unifies text / photo / video behind one durable, retryable path and needs
`chat_id` targeting in the queue **now** (not just for video).

**2a. Migration `0036_notification_queue_chat_id.sql`** — `ALTER TABLE notification_queue ADD COLUMN
chat_id bigint NULL`. `NULL` = the existing beacon-label path; set = send direct to that raw chat.

**2b. `queue::enqueue` gains `chat_id: Option<i64>`** (`queue.rs:28`) + `QueuedNotification.chat_id`
(`queue.rs:14`, read in `claim_next`). Timestamps/columns otherwise unchanged.

**2c. Consumer send selection** (`sentinel-notification/src/consumer.rs::process`, `consumer.rs:26-58`):
- Resolve config/token as today (`resolve_config`, `consumer.rs:61`).
- Download `image_key` / `video_key` from the object store (`media::get`) as today.
- **Target + method:**
  - `job.chat_id` set → send **direct** to that chat via `notification::client`: `send_video` if
    `video_key`, else `send_photo` if image, else `send_message`.
  - `job.chat_id` NULL → existing beacon path (`notification::dispatch(store, cfg, target, msg, image)`).
- `send_video` is new (Phase 4); until then a `video_key` with no `send_video` keeps the current
  "lands with #15" log. Backoff/retry via `mark_retry_or_fail` unchanged.

## Phase 3 — `Reply to sender` node (text + photo → queue)

Enqueues a reply to the **specific chat that triggered the run** (not a configured beacon). Async by
design (decision above): the node reports *queued*, not *delivered*.

| | |
|---|---|
| **Kind** | `NodeKind::ReplyToSender` — category **"Telegram"** (new category) |
| **Inputs** | `message*` (Text), `chat_id*` (Num), `image` (Image, optional) |
| **Outputs** | `queued` (Bool), `id` (Text — queue row id) |
| **Behavior** | If `image` wired → `media::put(&jpeg,"image/jpeg","jpg")` → `image_key` (mirrors `RemoteNotifier`, `ports.rs:178`). Then `queue::enqueue(db, tenant, target=None, message, image_key, video_key=None, chat_id=Some(chat_id))`. Requires the object store (present under the #57 `NOTIFY_QUEUE` deploy). |
| **Timeout** | 10 s (just an upload + insert). |

- `domain.rs`: variant + ports + category arm + slug/label + add to `ALL` (bump size).
- `node.rs`: dispatch arm → `node/telegram.rs::run_reply_to_sender` (new submodule); uses `sys.db`
  (the run's tenant `Db`) + `app_core::event_queue`/`notification::queue` + `media`.

---

## Phase 4 — `AI Answer` node (text-in → text-out)

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

## Phase 5 — Video reply (`Reply with recording`)

Reply to the sender with a **recording clip**, reusing the Phase 2 queue+chat_id path. The `chat_id`
column and consumer targeting already land in Phase 2, so this phase is just the video-specific bits:

1. **`send_video`** — add to `notification/client.rs` (multipart `sendVideo`, mirror `send_photo`
   `client.rs:34`); wire the video arm into the consumer's chat_id send-selection (Phase 2c).
2. **Edge clip fetch** — new GET on the edge + a `recording.rs` helper: `GET {edge_api}/recording/{clip_id}`
   → mp4 bytes (`resolve_edge_api_base`, `recording.rs:92`; only start/stop exist today).
3. **`Reply with recording` node** (`NodeKind::ReplyWithRecording`, "Telegram"): inputs
   `clip_id*` (Text, from `Start recording`'s `clip_id` output `domain.rs:646`), `chat_id*` (Num),
   `caption` (Text). Fetches clip → `media::put(bytes,"video/mp4","mp4")` →
   `queue::enqueue(chat_id=Some, video_key=Some)`. Outputs `queued:Bool`, `id:Text`.

*Phase 5 is the one slice that touches the edge. Phases 1-4 ship the interactive text/photo bot on
their own (the `chat_id` migration in Phase 2 is additive and harmless without video). Ship 1-4, then 5.*

---

## Phase 6 — Palette, invariants, docs

- **Node count**: `NodeKind::ALL: [NodeKind; 56]` (`domain.rs:685`) → bump to `59` (adds
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

## Decisions

1. **Reply transport — DECIDED: everything queued.** All replies (text / photo / video) go through the
   #57 `notification_queue`; no synchronous sends. Nodes report *queued*, not *delivered*. Drives the
   Phase 2 `chat_id` column + unified consumer send-selection.
2. **Enqueue scope — DECIDED: enqueue everything.** Every message from a paired chat is enqueued as a
   `telegram_message` event (still tenant-scoped to the poller's tenant + paired chat_ids). `fire_event`
   no-ops when a tenant has no handler graph; the event_queue GC retires processed rows.
3. **Ship boundary — DECIDED (default): split.** Phases 1-4 (text/photo bot) first; Phase 5 (video) as a
   second commit on the same card.
4. **AI backend — DECIDED (default): Anthropic direct.** `AI Answer` uses `ANTHROPIC_API_KEY` +
   `ANTHROPIC_MODEL` (default `claude-opus-4-8`), not the image-only RunPod sidecar. Confirm the key is
   set in the flow pod's env before Phase 4.

---

## Out of scope (v1)

- Inline keyboards / callback queries; multi-turn stateful conversations (each run is stateless).
- `/arm` `/disarm` (need a confirm flow); a `List cameras` node for rich `/status`.
- Full tool-use agentic loop in `AI Answer` (single-turn only; agentic reply is a follow-up linked to #16).
- Inbound **photos** from users (payload already supports an inline base64 image, but no node consumes it yet).
