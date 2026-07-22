# Default workflow templates — teaching curriculum — Implementation Plan

**Board card:** #136 Default workflow templates (starter curriculum for every tenant)
**Goal:** Ship a coherent set of **global** workflow templates that every tenant sees in the
templates picker — a progressive curriculum that teaches the software's capabilities. Each template
is self-documented on the canvas with the Display nodes (Title / Paragraph / Markdown) + Comments.

## Architecture (how "default templates for everyone" works)
Reuses the existing template system (card #102): a `workflows` row with `data.template=true` +
`data.global=true` is surfaced to **every** tenant by `repo::load_global_templates` (read
cross-tenant by flag); "Use template" clones it into the caller's tenant as a normal, disabled graph.

- **Owner:** a stable **`platform`** tenant (seed-created) holds the shared catalog. Tenants never
  see the tenant itself — only its global templates. The operator can *assume* the platform tenant
  (master → navigate-as-tenant) to **polish the templates visually** in the real editor.
- **Seeder:** `sentinel-control/src/seed_templates.rs`, called from `seed::run()` on control boot.
  Graphs are built via the validated domain API (`Graph::add_node`/`add_edge` — a mis-wire fails the
  build, not prod). Insert is **`ON CONFLICT (tenant_id, id) DO NOTHING`** (the `workflows` PK is the
  composite `(tenant_id, id)`) against a **stable per-template UUID** (`stable_id`), so a redeploy
  never clobbers the operator's later visual edits. To ship a
  revised template, give it a new stable id.
- No migration, no new table, no per-tenant seed — global templates ARE the shared catalog.

## The curriculum (6 levels, ~12 templates)
Each ships documented in-canvas (Title/Paragraph/Markdown + Comment) and grouped `Aprender`.

- **L0 Onboarding** — `#0 Comece aqui — tour do editor` (docs-only; Title/Markdown/Comment). **[shipped — pilot]**
- **L1 Fundamentals** — `#1 Alerta de pessoa` (Cron → frame → Person detection → Compare/If → Cooldown → Notify+image); `#2 Quem está na porta?` (snapshot → Describe image → Notify).
- **L2 Richer reactions** — `#3 Snapshot anotado` (Detect → For each → Break → Draw BBox/Text → Show image); `#4 Clipe em movimento` (On event motion → Delay Trigger → Start/Stop recording → Reply with recording).
- **L3 Telegram bot** — `#5 Bot de comandos` (On event telegram_message → String switch → snapshot → Reply to sender); `#6 Bot em linguagem natural` (AI Switch → AI Extract → snapshot → AI Answer → Reply).
- **L4 State & data** — `#7 Relatório diário` (Schedule → count → Variables + Dictionary Add Row → Format → Notify); `#8 Cruzamento de linha` (Region → On event line_crossed → Draw BBox → alert).
- **L5 Faces & smart home** — `#9 Reconhecimento facial` (On event face_detected → Extract embedding → Recognize Face → If found → Notify / Store Face unknown); `#10 Luz por presença` (zona entered/exited → Tuya Control on/off).
- **L6 Map & presence** — `#11 Presença viva no Mapa` (detect in zona → RecordPresence).

Capability coverage matrix: triggers (Manual/Cron/Schedule/event), cameras, vision+AI (detect,
describe, draw, faces, AI If/Switch/Answer/Extract), flow (If/switch/For each/Cooldown/Delay
Trigger/logic), events, recording, Telegram, variables+dictionaries, tracking/Region/zones, Tuya,
presence/Map, Display nodes — all represented.

## Status / phasing
- **Phase 1 (shipped):** seed infrastructure (`seed_templates.rs`, `store::tenant_id_by_slug`,
  platform tenant, idempotent insert) + the L0 onboarding pilot.
- **Phase 2 (shipped):** all L1–L6 functional templates authored in `templates()` (one builder fn
  each, stable ids 0x02–0x0C), each with a Title + Markdown doc node. Built via the validated domain
  API — the `templates_build_*` test asserts every edge's ports **and types** resolve (it caught a
  `Num→Text` mismatch → fixed with a `NumToText` node). Event-driven templates wire only `exec` from
  `When Event` (payload/data ports need the event schema at runtime); required camera/chat_id inputs
  are left for the operator to fill on clone, as documented in each canvas. **Operator visual pass +
  real-run validation pending.**

## Out of scope (v1)
- Advanced "recipes" (HTTP post, Flip flop, 2-frame Motion, Blur Outside Box, in-graph dictionary
  composition) — a later L7.
- Live-value Display nodes (Text/Gauge) in the docs — arrive with #135 Slice 2.
- Localization of template copy beyond pt-BR.

## Build spec (per template) — turnkey for `seed_templates.rs::templates()`
Each = a `template(stable_id, name, desc)` graph + `add_node`/`add_edge` (validated) + a Title/
Markdown doc node. `stable_id` 0x01 taken (onboarding); use 0x02.. next. `NodeKind`s below are the
exact enum variants (`crates/app-core/src/workflows/domain.rs`); ports/config per `docs/workflow-nodes.md`.

- **#1 Alerta de pessoa** — trigger `Cron{every_secs:2}`. `GetCurrentFrame`(memoize off) →`image`
  `PersonDetection` →`count` `Compare{op:">",b:0}` →`result` `If` →true `Cooldown{window_secs:300}`
  → `NotifyRecipient`(message + wire `image`). Teaches: Cron, live frame, detection, Compare/If,
  Cooldown, notify-with-image.
- **#2 Quem está na porta?** — trigger `Manual`. `CameraSnapshot` →`image` `DescribeImage`
  {question:"tem alguém na porta?"} →`description` `NotifyRecipient`(message=description). Teaches:
  snapshot, DescribeImage (vision LLM).
- **#3 Snapshot anotado** — `Manual`. `CameraSnapshot`/`GetCurrentFrame` → `ObjectDetection`{class}
  →`detections` `ForEach` →loop `BreakDetection` →`bbox` `DrawBbox`{style:"reticle"} →`image`
  `DrawText` → `ShowImage`. Teaches: ForEach, Break*, DrawBbox/DrawText, Bbox type, ShowImage.
- **#4 Clipe em movimento** — trigger event `On event("motion")`. `DelayTrigger` →start
  `StartRecording`; →stop `StopRecording` →`clip_id` `ReplyWithRecording`/`NotifyRecipient`.
  Teaches: DelayTrigger debounce, recording lifecycle.
- **#5 Bot de comandos** — `OnEvent("telegram_message")` (outputs text/chat_id/command/args) →
  `StringSwitch{arms:["/snap","/status"]}` →/snap `CameraSnapshot` →`image` `ReplyToSender`(wire
  chat_id + image). Teaches: system events, StringSwitch, ReplyToSender.
- **#6 Bot em linguagem natural** — `OnEvent("telegram_message")` → `AiSwitch{arms:[foto,pergunta,
  status]}` →foto `AiExtract{field:"camera name",type:Camera}` →`value` `CameraSnapshot`;
  pergunta `AiAnswer` →`response` `ReplyToSender`. Teaches: AiSwitch, AiExtract, AiAnswer.
- **#7 Relatório diário** — trigger `Schedule{freq:daily,times}`. per-camera `GetCurrentFrame` →
  `PersonDetection` →`count` `SetVariable`/`GetVariable(total)` → `DictAddRow`(log table) →
  `Format` → `NotifyRecipient`. Teaches: Schedule, Variables, Dictionary mutate, Format.
- **#8 Cruzamento de linha** — `Region{points: line}` shared into `OnEvent("line_crossed")` →
  `DrawBbox` → `CreateAlert`/`NotifyRecipient`. Teaches: Region (line/zone), edge-tracking events.
- **#9 Reconhecimento facial** — `OnEvent("face_detected")` → `FaceEmbedding` →`embedding`
  `RecognizeFace{top_k:5}` →`found` `If` →true `NotifyRecipient`; →false `StoreFace`(label=""
  unknown). Teaches: face pipeline (embed→kNN→union gallery), StoreFace.
- **#10 Luz por presença** — `OnEvent("object_entered")` zona → `TuyaControl`(switch on); paired
  `OnEvent("object_exited")` + empty → `TuyaControl`(off). Teaches: TuyaControl/Read, zones→action.
- **#11 Presença viva no Mapa** — `Cron`/event → detect person in zona → `RecordPresence`
  {category,label}. Teaches: RecordPresence, Map pulse.

Note: verify exact port names against the catalog at build time; `add_edge` will fail loudly on a
wrong port, which is the safety net. Some events (`motion`, `line_crossed`, `face_detected`,
`object_entered/exited`, `telegram_message`) are the system-event catalog (`repo::system_events()`).
