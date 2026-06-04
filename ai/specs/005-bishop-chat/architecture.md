# Architecture Notes — Bishop Chat

## Components

- `src/chat/domain.rs` — `Conversation` (log lines, input buffer, cursor, history, pending
  flag, typing state).
- `src/chat/handler.rs` — input handling, send, typewriter reveal.
- `src/chat/view.rs` — chat log + input bar.
- `src/agent/domain.rs` — `Message`, `Role`, `AgentConfig`, `AgentOutcome`, `text_of()`.
- `src/agent/client.rs` — HTTP POST to the Claude API, tool-call streaming.
- `src/agent/handler.rs` — background agent thread, message splicing.

## APIs

- Anthropic / Claude Messages API (model from `ANTHROPIC_MODEL`, key from
  `ANTHROPIC_API_KEY`).

## Data Flow

Operator submits → `send_to_bishop` spawns a thread that calls the Claude client → result
returns as an `AgentOutcome` over the `reply_tx`/`rx` channel (with an `ActivityGuard`) →
reply text is revealed one char per tick using `settings.typing_speed`.

## Important Decisions

- Background-thread + mpsc channel keeps the 50ms tick loop non-blocking.
- History lives in memory only — single session, local-first.

## Risks

- Network failures must be reported in-log; reveal must not block on slow replies.
