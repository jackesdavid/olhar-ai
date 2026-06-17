---
name: handoff
description: "Second brain for ANY work on this project (Sentinel). Save a session handoff or resume one. SAVE: capture what changed into docs — a rolling log (docs/HANDOFF.md) plus the relevant per-area reference (docs/ref/*.md) — with branch, commits, build status, what's pending, and blockers, then commit. RESUME: read the latest handoff + refs, check git, and continue the pending work. Invoke when the user says 'handoff this session', 'store/save progress', 'pick up where we left off', 'continue', 'what were we doing', or '/handoff'."
user-invocable: true
argument-hint: "[save | resume] [optional area/note]"
allowed-tools:
  - Read
  - Write
  - Edit
  - Bash
  - Grep
  - Glob
---

# Handoff — project second brain

Keep work resumable across sessions. The artifacts:

- **`docs/HANDOFF.md`** — the rolling log (newest entry on top). One dated entry per handoff: the
  durable, scannable memory of what happened and what's next.
- **`docs/ref/<area>.md`** — the deep per-area reference (architecture, conventions, file layout,
  and a living **Build status** section). Linked from `CLAUDE.md`.

Default to **save** unless the user clearly means **resume**.

## Save a handoff

1. **Gather state**: `git rev-parse --abbrev-ref HEAD`, `git status --short`, `git log --oneline -10`.
   Identify the area worked on and whether the tree/branch is pushed.
2. **Per-area ref**: if the session built/changed a substantial feature, create or update
   `docs/ref/<area>.md` (mirror the style of existing refs — overview, file layout, conventions,
   and a **Build status** section: done / pending / not-done / backlog). Add a one-line pointer in
   `CLAUDE.md`'s index if the doc is new.
3. **Rolling log**: prepend a new entry to `docs/HANDOFF.md` using the template below. Convert
   relative dates to absolute. Be honest — pending and blockers matter more than what's done.
4. **Verify claims** you record (build/tests/deploy) actually ran; if you skipped something, say so.
5. **Commit + push** the docs (no Claude attribution; end PR/commit bodies per project rules). Do
   **not** deploy or merge as part of a handoff unless explicitly asked.

### Entry template (prepend to docs/HANDOFF.md)

```
## <YYYY-MM-DD> — <area / headline>

- **Branch:** <name> (pushed? merged to new-ui?)
- **Commits:** <short shas / range>
- **Done:** <what shipped this session>
- **State:** <build/check/cargo results; what's verified vs not>
- **Pending / next:** <the next concrete steps>
- **Blockers:** <anything waiting on the user or external>
- **Refs:** docs/ref/<area>.md, key files/paths
```

## Resume a session

1. Read the **top entry** of `docs/HANDOFF.md` and the **Refs** it names (per-area `docs/ref/*.md`).
2. `git status --short` + `git log --oneline -10`; check out the named branch if needed.
3. Re-state the current position and the next pending item, then continue it. If the build state is
   unknown, re-verify before changing code.

## Conventions (all sessions)

Follow `CLAUDE.md`: additive RFC3339 migrations, emerald/dot-field identity + house style,
the edge is the only camera-touching component, no Claude attribution in commits/PRs. Deploy is
`task cloud-deploy` (see `docs/ref/deploy.md`) — confirm with the user first.
