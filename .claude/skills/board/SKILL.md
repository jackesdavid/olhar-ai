---
name: board
description: "Drive the team Kanban board — the Obsidian Kanban board at `docs/Task Board.md`, the SOURCE OF TRUTH. Turn a use case into a planned card, and move cards across columns as work progresses: plan → In Progress → Review → Done. Each card is a short \"#N title\" checklist item; detail lives in a plan note under docs/plans/, linked from the card. Triggers: 'plan this use case', 'move the card', 'start implementation', 'this is in review now', 'mark it done', '/board'."
user-invocable: true
argument-hint: "[plan|implement|review|done] [use-case name | card #N or keyword]"
allowed-tools:
  - Bash
  - Read
  - Edit
  - Write
  - Glob
  - Grep
---

# Task Board — `docs/Task Board.md` (Obsidian Kanban)

The board is the **Obsidian Kanban board** at `docs/Task Board.md`, and it is the **single source
of truth**. A card is a **markdown checklist item** under a column heading. This skill creates
cards and moves them by editing that one file.

> **GitHub Project #3 is no longer the board.** Do not create issues or edit project items for
> board work. Cards #148–#154 also exist as GitHub issues from the period when GitHub was truth —
> leave them alone; this file is what counts now.

## File shape

```markdown
---

kanban-plugin: board

---

## Backlog
## Bugs
## ToDo
## In Progress
## Review
## Done

**Complete**
- [x] #119 …

%% kanban:settings
```{"kanban-plugin":"board", …}```
%%
```

**Never touch the trailing `%% kanban:settings … %%` block.** It is the plugin's own config;
corrupting it breaks the board view. Leave it byte-for-byte as found, at the end of the file.

Columns are separated by **two blank lines**. Match the existing spacing when inserting.

## Columns

| Column | Meaning | Checkbox |
|---|---|---|
| `## Backlog` | not scheduled | `- [ ] ` |
| `## Bugs` | defects, any priority — the column **is** the marker, so **no `🐛` prefix** | `- [ ] ` |
| `## ToDo` | ready to pick up | `- [ ] ` |
| `## In Progress` | being worked now | `- [ ] ` |
| `## Review` | done, awaiting verification | `- [ ] ` |
| `## Done` | shipped — goes **below the `**Complete**` line** | `- [x] ` |

## Card format

```markdown
- [ ] #N <short significant title>
	  - Plan: [[plan-note-slug]]
```

- **Title**: `#N <short significant title>` — what it *is*, skimmable. Nothing else on that line.
- **`#N`**: a stable integer id, assigned at creation, **never** changed or reused.
- **The `Plan:` line is optional** and indented with a **TAB then two spaces**. Copy the
  indentation from a neighbouring card rather than retyping it — the plugin is picky, and a space
  where a tab belongs silently detaches the line from its card.
- Use an Obsidian `[[wikilink]]`: the plan note's filename without `.md`. Obsidian resolves it from
  anywhere in the vault, so no path is needed. A few cards use `Use case:` or `Epic:` instead —
  same shape.
- Detail lives in the plan note, never on the card. No prose, refs, branch notes or file paths.

## Card numbers

Scan every `#N` on the board; the new card is `#(highest + 1)`:

```sh
grep -oE '#[0-9]+' "docs/Task Board.md" | tr -d '#' | sort -n | tail -1
```

Never reuse or renumber. Match a card by the exact `#N ` fragment — the trailing space
disambiguates `#8 ` from `#80 `.

## Stages (the argument)

Read the first word of the argument; infer from the conversation if absent.

| Arg | Action |
|-----|--------|
| `plan` | Draft a codebase-grounded plan note → `docs/plans/<slug>.md`, then add a card to `## ToDo`. |
| `implement` / `start` | Move the card to `## In Progress`; ensure its plan note exists/refreshed. |
| `review` | Move the card to `## Review`. |
| `done` | Move the card to `## Done` below `**Complete**`, and flip `- [ ]` → `- [x]`. |

### Move a card
Read the file, then cut the card's lines — the `- [ ]` line **and** its indented `Plan:` line if it
has one — from the current column and paste them into the target column. Two rules:

- **Keep the pair together.** An orphaned `Plan:` line renders as a stray card.
- **Only `## Done` flips the checkbox.** Every other column keeps `- [ ]`.

Then confirm in one line, e.g. "Moved #119 → Review."

### Plan a card
1. **Find + read the use case** in `docs/use-cases/` (fuzzy-match the name).
2. **Ground the plan in the codebase** (Glob/Grep/Read) — name real modules/crates/nodes/tables,
   reuse existing infra, honour project memory.
3. **Write the plan** to `docs/plans/<slug>.md` (`<slug>` kebab-case): phases (ordered, buildable
   slices), open decisions, out-of-scope.
4. **Add the card** to `## ToDo` with the next `#N` and a `Plan: [[<slug>]]` line.
5. Several clearly-independent features → one card each, each with its own note.
6. For `implement` with **no** existing card → create one directly in `## In Progress`.

### Which card
After the stage word, the rest is a card ref — match by `#N` or fuzzy title against the board. No
ref → the task being actively worked in **this** conversation. Several plausible + unsure →
`AskUserQuestion` with the candidate titles.

## The plan note

`docs/plans/<slug>.md`:

```markdown
# <Title> — Implementation Plan
**Use case:** [[<use-case-slug>]]   **Board card:** #N <title>
**Goal:** <one line>
## Phase 1 — <slice>
- concrete steps naming real files/modules
## Open decisions
1. …
## Out of scope (v1)
- …
```

`[[wikilinks]]` are fine again now that the board is Obsidian. They do **not** render on GitHub, so
use plain repo-relative links for anything a non-Obsidian reader needs to follow.

## Commit the change

`docs/Task Board.md` is a tracked file, so a board move is a commit like any other — commit it
(plus any new plan note) rather than leaving it dirty in the working tree.

**The vault is local.** Committing is what makes a board change visible to anyone else; if a
collaborator needs to see the state, the commit is not optional. This is the one real cost of
moving off the shared GitHub Project — worth saying out loud when board state matters to someone
other than the operator.
