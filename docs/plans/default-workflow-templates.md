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
  platform tenant, idempotent insert) + the L0 onboarding pilot. Validates the whole pipeline
  (seed → picker → editor) before mass-authoring.
- **Phase 2+:** author the L1–L6 functional templates into `templates()` (one builder fn each),
  batch by batch. The operator gives each a visual pass in-editor afterwards.

## Out of scope (v1)
- Advanced "recipes" (HTTP post, Flip flop, 2-frame Motion, Blur Outside Box, in-graph dictionary
  composition) — a later L7.
- Live-value Display nodes (Text/Gauge) in the docs — arrive with #135 Slice 2.
- Localization of template copy beyond pt-BR.
