---
name: board
description: "Drive the Kanban board in docs/Task Board.md: turn a use case in docs/use-cases/ into a planned ToDo card, and move cards across columns as work progresses. Every card is a short, significant title linked to a plan note under docs/plans/ — all detail lives in the note, never inline on the card. Use when planning a use case, starting implementation (→ In Progress), starting visual/UI review (→ Review), or finishing with no more changes (→ Done). Triggers: 'plan this use case', 'move the card', 'start implementation', 'this is in review now', 'mark it done on the board', '/board'."
user-invocable: true
argument-hint: "[plan|implement|review|done] [use-case name | card title or keyword]"
allowed-tools:
  - Read
  - Edit
  - Glob
  - Grep
---

# Task Board — card lifecycle

Advances a card through the Kanban board at `docs/Task Board.md` as a task moves through its
stages, keeping each card a short title linked to a plan note in `docs/plans/`. This is a
repeated, hands-off chore: invoke it at each transition and it edits the board for you.

## The board

`docs/Task Board.md` is an **Obsidian Kanban** file. Columns are `## ` headings; cards are
checkbox list items under them. The lifecycle columns this skill touches, in order:

```
## ToDo          →  ## In Progress  →  ## Review  →  ## Done
```

A **card** is one list item line — `- [ ] #N <title>` — plus its 2-space-indented link line(s)
beneath it. Moving a card means moving the whole block (title **and** its links).

Other columns (`## Bugs`, `## Backlog`) and the `**Complete**` marker under `## Done` exist too;
leave them alone unless a card you're moving lives there.

## Card numbers — always, forever

**Every card must have a `#N` number prefix — always.** This is a stable, monotone integer ID
assigned at creation and never changed, even when the card moves columns or gets renamed.

- **Format**: `- [ ] #N Card title here` (the `#N` is the first thing after `- [ ] `).
- **Assigning a number**: before creating a new card, scan all cards on the entire board (all
  columns, including Done) for the highest existing `#N`. The new card gets `#(highest + 1)`.
- **Matching by number**: if the user references `#5` or `"card 5"`, find the card with `#5`
  directly — no fuzzy match needed.
- **Never reuse or renumber**: if a card is deleted, its number is retired. Don't renumber.
- **Missing numbers on old cards**: if you encounter a card without a `#N`, assign the next
  available number to it before doing anything else with that card.

## What a card holds — title + links only

**This is the cardinal rule.** A card is a **`#N` number + short, significant title** plus,
indented beneath it, **only wikilinks** — never prose, context, rationale, or steps. All of
that knowledge lives in the card's **plan note** under `docs/plans/`.

A well-formed card:

```
- [ ] #12 Workflow runs: real-time over SSE
	  - Plan: [[workflow-runs-realtime]]
```

…optionally with a `- Use case: [[<slug>]]` line when one exists. That's the whole card.

- **Number**: `#N` prefix, stable across the card's entire life. Never omit it.
- **Title**: one line after the number, the significant essence — what it *is*, not how/why. Skimmable.
- **No inline knowledge**: root cause, phases, refs, "why", branch notes, file paths — none of
  it goes on the card. It goes in the plan note.
- **Whenever you create or touch a card** and find detail that belongs to it, write that detail
  into its `docs/plans/<slug>.md` note and leave only the `[[…]]` link on the card. If the card
  has no note yet, create one (see "The plan note").
- The only per-card text besides the number+title is the wikilink line(s). Keep `Ref:`/`Branch:`/`Why:`
  lines **inside the plan note**, never on the card.

## Stages (the argument)

Read the first word of the argument. If absent, infer it from the conversation (what just
happened / what the user said).

| Arg | Meaning | Action |
|-----|---------|--------|
| `plan` | Turn a use case into a planned card | Read a use case from `docs/use-cases/`, draft a codebase-grounded plan, **write it to a vault note** in `docs/plans/`, and **create a card in ToDo** that links to it (see "Planning from a use case"). |
| `implement` / `start` | We're starting to build it | Move the card to **In Progress** and make sure it **links to its plan note** (create/refresh the note from the plan you're about to execute). See "The plan note". |
| `review` | We're starting visual/UI review | Move the card to **Review**. No code/notes changes. |
| `done` | Reviewed, nothing more to touch | Move the card to **Done** and flip `- [ ]` → `- [x]`. |

## Planning from a use case (`plan` stage)

`/board plan <name>` takes a use case and produces a ready-to-pick ToDo card.

1. **Find the use case.** Fuzzy-match `<name>` against files in `docs/use-cases/` (e.g.
   `school-frequence` → `docs/use-cases/school-frequence.md`). Read it in full.
2. **Ground the plan in the codebase.** Before writing steps, explore: which existing modules,
   crates, nodes, tables, and patterns the feature should build on (use Glob/Grep/Read). A plan
   that names real files and reuses existing infra beats an abstract one. Honour project memory
   (e.g. keep agent tools in sync, persist to Postgres+pgvector, no window.prompt in the WebView).
3. **Draft the plan.** A phased, ordered breakdown — each phase a concrete, buildable slice.
   Prefer vertical slices that ship value. Call out:
   - **Phases / steps** — what to build, in order, naming real modules/files where known.
   - **Open decisions** — the genuinely TBD architecture choices the use case leaves open
     (don't silently pick; list them so they can be decided). If the use case says "grill me",
     surface the sharpest 2–4 questions here rather than guessing.
   - **Out of scope (v1)** — what you're deliberately deferring.
4. **Write the plan to the vault** (not inline in the card). Create `docs/plans/<slug>.md` — a
   proper Obsidian note (see "The plan note" for its shape). `<slug>` is a kebab-case name derived
   from the use case (e.g. `school-student-frequency`).
5. **Create the card in ToDo** that *links* to the plan. Assign the next available number,
   title it from the use case (concise), and give it just two note lines — a wikilink to the
   plan and a wikilink to the use case:
   ```
   - [ ] #23 School Student Frequency Case
     - Plan: [[school-student-frequency]]
     - Use case: [[school-frequence]]
   ```
6. If the use case is clearly several independent features, it's fine to create **one card per
   slice** (each linking its own plan note), but default to a single card unless the split is obvious.

Note: a card created by `plan` lands in **ToDo**. Later, `/board implement` moves it to In Progress
and refreshes the linked plan note with the concrete plan you execute.

## Which card

After the stage word, the rest of the argument (if any) is a title keyword — fuzzy-match it
against existing card titles. If there's no keyword:

- Use the task being actively worked on in **this conversation** (the most recent feature/fix).
- If that maps to an existing card, move that one.
- If several cards plausibly match and you can't tell, ask with `AskUserQuestion` (list the
  candidate titles).
- For `implement` only: if **no** matching card exists anywhere on the board, **create** one
  directly in **In Progress** with a concise title, then link its plan note.

## The plan note (vault-linked, not inline)

Plans live as their own notes in the Obsidian vault under `docs/plans/<slug>.md`, and the card
just **links** to them with a wikilink — `[[<slug>]]`. Never paste the full plan into the card
body; keep cards skimmable.

The note is a normal markdown document. Recommended shape:

```markdown
# <Title> — Implementation Plan

**Use case:** [[<use-case-slug>]]
**Board card:** <card title> (<column>)
**Goal:** <one line>

## Phase 1 — <slice>
- concrete steps naming real files/modules

## Phase 2 — <slice>
…

## Open decisions (grill before building)
1. …

## Out of scope (v1)
- …
```

- **`plan`**: write this note fresh from the use case + codebase grounding, then create the
  linking card in ToDo.
- **`implement`**: ensure the linked note exists and reflects the plan you're about to execute —
  create it if the card has none, or refresh its phases if the approach changed — then move the
  card to In Progress. The card keeps its `Plan: [[<slug>]]` line; only the note changes.
- Cross-link: the plan note links back to its use case (`[[…]]`) and names the card.

## Editing safely

- **Do not** touch the YAML frontmatter (`kanban-plugin: board`) or the `%% kanban:settings %%`
  block at the bottom — they keep the board valid.
- Preserve the existing blank-line spacing: columns are separated by blank lines, and there are
  two blank lines before the next `## ` heading. Match the surrounding style.
- Move the card by **removing** its block from the source column and **inserting** it at the end
  of the target column's card list. Carry its link line(s) with it.
- **Never leave inline knowledge on a card.** If you encounter a card carrying prose/notes,
  migrate that text into its `docs/plans/<slug>.md` note and reduce the card to title + `[[…]]`.
- Keep the card title text (including its `#N` prefix) byte-for-byte across a move (only `[ ]`→`[x]` may change, for `done`).
- Use `Edit` against `docs/Task Board.md`. After editing, confirm to the user in one line which
  card moved and to where (e.g. "Moved 'Image node' → Review").

## GitHub Project mirror (shared with the co-founder)

`docs/Task Board.md` stays the **source of truth**, but the board is MIRRORED to a shared GitHub
Project the co-founder can see: **user `jackesdavid`, Project #3 "OAI Board"**
(`github.com/users/jackesdavid/projects/3`). **After every board edit below, also update the GitHub
Project** so the mirror never drifts.

**Stable refs** (single-select `Status` field = the columns):

| Obsidian column | GitHub Status | option id |
|---|---|---|
| Backlog | Backlog | `f75ad846` |
| ToDo | Ready | `61e4505c` |
| In Progress | In progress | `47fc9ee4` |
| Review | In review | `df73e18b` |
| Done | Done | `98236657` |
| Bugs | Ready (prefix title `🐛 `) | `61e4505c` |

- project id `PVT_kwHOAE3EiM4BeByU` · Status field id `PVTSSF_lAHOAE3EiM4BeByUzhYe_68`.
- **Auth first**: `gh auth switch --user jackesdavid` (the account with the `project` scope + repo
  access; the CLI otherwise flips to `jdvlemos-fitec`, which 404s).

**Targeted update — the DEFAULT for a single card** (fast, preserves the rest of the Project):
- **`plan`** (new card → ToDo/Ready): create it and set Status.
  ```sh
  ID=$(gh project item-create 3 --owner jackesdavid --title "#N <title>" --format json --jq .id)
  gh project item-edit --id "$ID" --project-id PVT_kwHOAE3EiM4BeByU \
    --field-id PVTSSF_lAHOAE3EiM4BeByUzhYe_68 --single-select-option-id 61e4505c
  ```
- **`implement` / `review` / `done`** (card changes column): find the item by its `#N ` prefix
  (exact, trailing space — so `#12 ` never matches `#120`), then set the target column's option.
  ```sh
  ID=$(gh project item-list 3 --owner jackesdavid --format json --limit 500 \
       --jq '.items[] | select(.title | startswith("#N ")) | .id')
  gh project item-edit --id "$ID" --project-id PVT_kwHOAE3EiM4BeByU \
    --field-id PVTSSF_lAHOAE3EiM4BeByUzhYe_68 --single-select-option-id <target option id>
  ```
  (A card without a `#N` prefix — a few legacy ones — matches on its full title instead.)

**Full resync — only for bulk reconciliation**: `python3 .claude/skills/board/sync-github-project.py`
wipes the Project and recreates every card from `docs/Task Board.md`. ⚠️ It erases any edit made
directly in the GitHub Project, so prefer the targeted update above for normal card moves.

## Flow recap

0. `/board plan <use-case>` — read the use case, create a planned card in ToDo.
1. `/board implement` — card → In Progress, plan written/refreshed in its linked note.
2. (you write the code) …
3. `/board review` — card → Review when visual/UI review begins.
4. (you eyeball the UI, tweak if needed) …
5. `/board done` — card → Done; don't touch it again.

Every step edits `docs/Task Board.md` **and** mirrors the change to GitHub Project #3 (see §GitHub
Project mirror) — the targeted `gh project` update, not a full resync.
