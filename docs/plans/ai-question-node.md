# AI Branch Node — Implementation Plan

**Board card:** #19 AI If node — yes/no question routes exec like If (In Progress)
**Node:** `AskAi` / slug `ask_ai` / label **"AI Branch"** / category "AI"
**Goal:** A workflow node that asks **our agent** (full tool-use loop, all context tools) to
make a yes/no decision, and routes exec to `true` or `false` — like `If`, but the condition is
evaluated by Sentinel's own AI with access to everything it can already do.

---

## Current state (what already exists)

⚠️ **This node is already implemented end-to-end** — but against a *one-shot vision call*, not
the agent. This plan is a **re-architecture**, not a greenfield build. Already wired:

- `domain.rs` — `AskAi` in `NodeKind::ALL`, ports (`ASK_AI_IN` = image + question), `EXEC_OUT_IF`
  (`true`/`false`), category "AI", label "AI Branch", `default_config` = `{question, timeout_secs}`.
- `node/ai.rs::run_ask_ai` — calls `sys.vision.ask_yes_no(question, image)` and stores `result: bool`.
- `engine.rs` — `forward_epoch` routes `AskAi` by the `result` bool (same path as `If`); tests use
  `StubVision` + `ctx_with_vision(bool)`.
- `ports.rs` — `Vision::ask_yes_no` trait method → `agent::vision::ask_yes_no`.
- `agent/vision.rs::ask_yes_no` — single Anthropic call, `system = "Answer ONLY YES or NO"`,
  `max_tokens: 8`, parses `text.starts_with('Y')`.

**The gap vs. what we want:** today it's a dumb one-shot describe. No tools, no context, no
freedom to instruct the AI, and the bool comes from fragile text parsing.

---

## What we're changing

1. **Use OUR agent, not a separate one.** Replace the one-shot `ask_yes_no` with the real
   tool-use loop (`agent::client`) driven by `ToolRegistry::new()` + the node's existing
   `ToolContext` (`sys`). The AI can then do everything it already does in chat —
   list cameras, grab a snapshot, describe a feed, check alerts, etc. — *while deciding*.
2. **A freedom textarea (`instructions`).** The primary config field: a multi-line prompt where
   the user tells the AI **what to decide and how**. This is the bulk of the equation.
3. **The `question`/input is only *part* of the equation.** The wired/literal `question` (and the
   optional `image`) are the *dynamic* values merged into the run; `instructions` is the standing
   task. Both feed the same turn.
4. **Guaranteed boolean.** The agent must finish by calling a terminal **`decide` tool**
   (`{ value: bool, reason: string }`). We read `value` straight from the tool call — no text
   parsing. Robust true/false, every time.

---

## Phase 1 — Agent: a decision tool-loop  `agent/client.rs`

Add `decide(...)` alongside `ask(...)` — a tool-use loop that returns a strict bool.

```rust
pub struct Decision { pub value: bool, pub reason: String }

/// Run the agent with the full tool registry until it commits a yes/no decision
/// via the injected `decide` tool. `instructions` is the standing task (freedom
/// textarea); `question` is the dynamic value; `image` is an optional frame.
pub fn decide(
    instructions: &str,
    question: Option<&str>,
    image: Option<&[u8]>,
    registry: &ToolRegistry,
    ctx: &ToolContext,
    timeout: Duration,
) -> Result<Decision, String>;
```

Implementation notes — mirror `ask()`'s loop, with three changes:
- **System prompt:** personality + a decision instruction ("You are evaluating a condition for an
  automation. Use your tools to gather whatever you need, then call `decide` exactly once with
  `value` true/false and a one-line `reason`. Do not reply in prose.") + `TOOL_HINT`.
- **First user turn:** optional image block (base64 JPEG, reuse `vision.rs` resize logic) +
  `instructions` + (if present) `Dynamic input: {question}`.
- **Tools:** `registry.specs_json()` **plus** an appended `decide` spec
  (`{ value: boolean (required), reason: string }`). On each assistant turn, if a `decide`
  `tool_use` block is present, capture `value`/`reason` and **return** (don't run it through
  `registry.run`). All *other* tool calls execute normally via `registry.run`, exactly like `ask`.
- **Forcing the finish:** allow up to `MAX_TOOL_ITERS` normal rounds. If the loop ends without a
  `decide` call, do one final request with `tool_choice: {type:"tool", name:"decide"}` so a bool
  is guaranteed. If even that fails → `Err`.
- Honour `timeout` on the ureq agent.

---

## Phase 2 — Node: call the agent  `workflows/node/ai.rs`

Rewrite `run_ask_ai` to drive the agent instead of `sys.vision.ask_yes_no`:

```rust
pub(super) fn run_ask_ai(config: &Value, inputs: &PortValues, sys: &ToolContext)
    -> Result<PortValues, String>
{
    let instructions = config.get("instructions").and_then(Value::as_str).unwrap_or("").trim();
    let question = input_text(inputs, config, "question");          // wired or literal, optional now
    let image = inputs.get("image").and_then(PortValue::as_image);
    if instructions.is_empty() && question.as_deref().unwrap_or("").is_empty() {
        return Err("AI Branch needs instructions (or a wired question) describing what to decide.");
    }
    let timeout = Duration::from_secs(config_u64(config, "timeout_secs").unwrap_or(65));
    let registry = ToolRegistry::new();
    let decision = sys.decider.decide(instructions, question.as_deref(), image, &registry, sys, timeout)?;
    let mut o = PortValues::new();
    o.insert("result".into(), PortValue::Bool(decision.value));
    // surface decision.reason to the run log so operators see *why* it branched
    Ok(o)
}
```

Engine routing is **unchanged** — it already branches on `result: bool`.

---

## Phase 3 — Testability seam  `ports.rs`

`client::decide` hits the network, so the unit tests that route `AskAi` need a stub (today they
stub `Vision`). Add a swappable port, mirroring how `ctx.vision` / `ctx.frames` already work:

```rust
pub trait Decider: Send + Sync {
    fn decide(&self, instructions: &str, question: Option<&str>, image: Option<&[u8]>,
              registry: &ToolRegistry, ctx: &ToolContext, timeout: Duration) -> Result<Decision, String>;
}
```

- Default impl (`LocalDecider`) → `agent::client::decide`.
- Add `decider: Arc<dyn Decider>` to `ToolContext`, default-populated in `new_local`.
- **Retire `Vision::ask_yes_no`** (trait method + `vision.rs::ask_yes_no`) — nothing else uses it.
- Tests: replace `StubVision`/`ctx_with_vision` with a `StubDecider(bool)` and
  `ctx_with_decider(bool)`; keep the existing true/false/missing routing assertions.

> Note on `ToolContext` cycle: `Decider::decide` takes `&ToolContext` but lives *inside* it.
> That's fine — the trait object doesn't hold a `ToolContext`, it just receives one at call time.

---

## Phase 4 — Config & ports  `workflows/domain.rs`

- `default_config(AskAi)` → `json!({ "instructions": "", "question": "", "timeout_secs": "65" })`.
- Keep `ASK_AI_IN` = `image` (optional) + `question` (optional Text, wired or literal).
- `instructions` is **config-only** (not a port) — it's the standing task, edited in the inspector.
- Label stays "AI Branch"; exec outs stay `true`/`false` (`EXEC_OUT_IF`); timeout 65 s.
- Update the doc-comment on the `AskAi` variant: it now runs the full agent with tools, not a
  one-shot vision call.

---

## Phase 5 — UI: the freedom textarea  `sentinel-app/src/routes/workflows/+page.svelte`

The inspector renders config fields by key/port heuristics with a plain `<Input>` fallback. Add a
branch so `instructions` renders a **multi-line `<Textarea>`** (the freedom field):

```svelte
{:else if key === "instructions"}
  <Textarea
    class="min-h-24 text-xs"
    placeholder="Describe what the AI should decide — e.g. 'Look at the camera; answer true only if a delivery van is parked in the driveway.'"
    disabled={editLocked}
    value={String(selectedNode.config[key] ?? "")}
    oninput={(e) => liveSaveConfig(selectedNode, key, e.currentTarget.value)} />
```

- Import `Textarea` from `$lib/components/ui/textarea` (add the shadcn component if absent).
- `question` keeps the existing `<Input>` fallback (it's the short dynamic value).
- No catalog change needed — palette/inspector auto-derive from `NodeKind::ALL`.

---

## Phase 6 — Docs & memory

- Update `docs/workflow-nodes.md` AI Branch entry: now an agentic decision (tools + context),
  `instructions` + optional `question`/`image`, true/false routing.
- `crates/app-core/src/agent/tools.rs` is the *chat* agent's UI-effect channel — workflow nodes
  auto-derive, so no change there (per the keep-agent-tools-in-sync memory, this node reuses the
  existing registry rather than adding tools).

---

## Open decisions (grill before building)

1. **Show the `reason`?** Recommend yes — log `decision.reason` to the run log so operators see
   *why* it went true/false. Cheap, high value. (Not a data output port in v1.)
2. **Tool budget.** The agent can now snapshot/describe inside a decision → slower + costs tokens.
   Cap at `MAX_TOOL_ITERS` (reuse the 6 from `ask`)? Make it configurable? Recommend: reuse 6,
   defer config.
3. **Which tools?** Full `ToolRegistry::new()` (cameras, snapshot, describe, alerts, notify)?
   Notify/raise-alert as a *side effect during a decision* is surprising. Recommend: full registry
   for v1 (matches "our agent with all our tools"), revisit if alert/notify-during-decide bites.
4. **Decider seam vs. inline.** Add the `Decider` port (Phase 3) for testability, or keep tests
   network-free another way? Recommend the port — consistent with `vision`/`frames`.
5. **Keep text-only path?** With no image wired, the agent can still grab one via tools if the
   instructions imply it. Confirm that's desired (it's strictly more capable).

## Out of scope (v1)

- Multi-choice / 3-way routing (true/false/unknown)
- Exposing `confidence` or `reason` as data output ports
- Per-node model override (uses `ANTHROPIC_MODEL`, same as chat)
- Per-node custom tool allow-list
