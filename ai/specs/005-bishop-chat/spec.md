# Spec: Bishop Chat (Conversational AI)

> **Status: SHIPPED — retrospective record.** Reverse-engineered from `src/chat/` and
> `src/agent/`. Documents existing behavior; no code change implied.

## Goal

The operator holds a text conversation with "Bishop", the in-app AI synthetic powered by
Claude.

---

## User Value

A single in-console assistant that answers questions and (via tools, spec 006) acts on the
operator's cameras and alerts, so the operator can delegate work conversationally.

---

## Requirements

- Operator types a message; it is sent to the Claude API on a background thread.
- Bishop's reply reveals one character per tick (teletype), at a configurable speed.
- Conversation history is kept for the session and shown in a scrolling log.
- API calls never block the UI; the top bar shows PROCESSING while a reply is pending.
- Model is configurable via `ANTHROPIC_MODEL` (default `claude-opus-4-8`).

---

## Acceptance Criteria

- [x] Sending a message produces a Bishop reply rendered with the teletype effect.
- [x] The UI stays responsive while a reply is in flight (background thread + channel).
- [x] Typing speed follows the operator's config setting.
- [x] Missing `ANTHROPIC_API_KEY` is handled without crashing the console.
- [x] Conversation history persists across turns within a session.

---

## Dependencies

- 012-config (typing speed)

---

## Risks

- API latency/failures must surface gracefully in the log, not stall the UI.
- Long replies must reveal smoothly without blocking the tick loop.

---

## Notes

Out of scope: persisting chat history to disk. Tool use and vision are spec 006. Voice
output is spec 007.
