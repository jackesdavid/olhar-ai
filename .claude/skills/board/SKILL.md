---
name: board
description: "Drive the team Kanban board — GitHub Project #3 \"OAI Board\" (github.com/users/jackesdavid/projects/3), shared with the co-founder and the SOURCE OF TRUTH. Turn a use case into a planned card, and move cards across columns (the Status field) as work progresses: plan → In progress → In review → Done. Each card is a short \"#N title\" draft item; detail lives in a plan note under docs/plans/, linked from the card body. Triggers: 'plan this use case', 'move the card', 'start implementation', 'this is in review now', 'mark it done', '/board'."
user-invocable: true
argument-hint: "[plan|implement|review|done] [use-case name | card #N or keyword]"
allowed-tools:
  - Bash
  - Read
  - Edit
  - Glob
  - Grep
---

# Task Board — GitHub Project #3

The board is **GitHub Project #3 "OAI Board"** (`https://github.com/users/jackesdavid/projects/3`),
shared with the co-founder, and it is the **single source of truth**. A card lives as a Project
**draft item**; its **column is the item's `Status` field**. This skill creates cards and moves them
by editing that field via the `gh` CLI.

> The old Obsidian file `docs/Task Board.md` is **retired** — do NOT read or edit it. GitHub is truth.

## Auth + stable refs

**Always run first** — the `gh` CLI otherwise flips to `jdvlemos-fitec`, which 404s on the project:
```sh
gh auth switch --user jackesdavid
```

A card's **column IS its `Status` option**:

| Column | Status option | option id |
|---|---|---|
| Backlog | Backlog | `f75ad846` |
| ToDo | Ready | `61e4505c` |
| In Progress | In progress | `47fc9ee4` |
| Review | In review | `df73e18b` |
| Done | Done | `98236657` |
| Bugs | Ready + title prefix `🐛 ` | `61e4505c` |

- owner `jackesdavid` · project number `3` · **project id** `PVT_kwHOAE3EiM4BeByU`
  · **Status field id** `PVTSSF_lAHOAE3EiM4BeByUzhYe_68`.

## What a card holds — `#N` + title only

- **Title**: `#N <short significant title>` — what it *is*, skimmable. Nothing else in the title
  (bug cards additionally carry a leading `🐛 `).
- **`#N`**: a stable integer id (title prefix), assigned at creation, **never** changed or reused.
- **Detail lives in a plan note** `docs/plans/<slug>.md` (grounded, phased) — NOT on the card. The
  card's **body** carries one link to that note so the co-founder can open it. No prose, refs,
  branch notes, or file paths in the title/body beyond that link.

## Card numbers

Scan every item's title for the highest `#N`; the new card is `#(highest + 1)`:
```sh
gh project item-list 3 --owner jackesdavid --format json --limit 500 \
  --jq '[.items[].title | capture("#(?<n>[0-9]+)") | .n | tonumber] | max'
```
Never reuse/renumber. Match `#5` / "card 5" by the exact `#5 ` fragment (see below) — the trailing
space disambiguates `#8 ` from `#80 `, and `contains` still matches a `🐛 `-prefixed bug title.

## Stages (the argument)

Read the first word of the argument; infer from the conversation if absent.

| Arg | Action |
|-----|--------|
| `plan` | Draft a codebase-grounded plan note → `docs/plans/<slug>.md`, then **create a card** (draft item) in **Ready**, body linking the note. |
| `implement` / `start` | Set the card's Status → **In progress**; ensure its plan note exists/refreshed. |
| `review` | Set Status → **In review**. |
| `done` | Set Status → **Done**. |

### Move a card (`implement` / `review` / `done`)
Find the item by its `#N ` fragment, set the target Status, confirm in one line:
```sh
ID=$(gh project item-list 3 --owner jackesdavid --format json --limit 500 \
     --jq '.items[] | select(.title | contains("#N ")) | .id')
gh project item-edit --id "$ID" --project-id PVT_kwHOAE3EiM4BeByU \
  --field-id PVTSSF_lAHOAE3EiM4BeByUzhYe_68 --single-select-option-id <target option id>
```
Then tell the user e.g. "Moved #119 → In review."

### Plan a card (`plan`)
1. **Find + read the use case** in `docs/use-cases/` (fuzzy-match the name).
2. **Ground the plan in the codebase** (Glob/Grep/Read) — name real modules/crates/nodes/tables,
   reuse existing infra, honour project memory.
3. **Write the plan** to `docs/plans/<slug>.md` (`<slug>` kebab-case): phases (ordered, buildable
   slices), open decisions, out-of-scope. A normal markdown doc — plain repo links, **no
   `[[wikilinks]]`** (they don't render on GitHub).
4. **Create the card** in Ready, body linking the note:
   ```sh
   N=<next number>; SLUG=<slug>
   ID=$(gh project item-create 3 --owner jackesdavid --format json --jq .id \
        --title "#$N <title>" \
        --body "Plan: https://github.com/jackesdavid/Sentinel/blob/main/docs/plans/$SLUG.md")
   gh project item-edit --id "$ID" --project-id PVT_kwHOAE3EiM4BeByU \
     --field-id PVTSSF_lAHOAE3EiM4BeByUzhYe_68 --single-select-option-id 61e4505c   # Ready
   ```
5. Several clearly-independent features → one card each (each with its own note).

## Which card
After the stage word, the rest is a card ref — match by `#N` or fuzzy title against the Project
items (`gh project item-list 3 --owner jackesdavid --format json --limit 500`). No ref → the task
being actively worked in **this** conversation. Several plausible + unsure → `AskUserQuestion` with
the candidate titles. For `implement` with **no** existing card → create one directly in **In
progress**, body linking its plan note.

## The plan note (repo markdown)
`docs/plans/<slug>.md` — a normal markdown doc:
```markdown
# <Title> — Implementation Plan
**Use case:** docs/use-cases/<slug>.md   **Board card:** #N <title>
**Goal:** <one line>
## Phase 1 — <slice>
- concrete steps naming real files/modules
## Open decisions
1. …
## Out of scope (v1)
- …
```
Committed with the code (the co-founder reads it on GitHub). The card **body** links to it.
