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
## Specification
## Generation
## Validation
## Revision
## Audit
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

Work moves through a **pipeline**, in this order. Each stage has one job, and doing another
stage's job inside it is the failure mode the pipeline exists to prevent.

| Column | What it is | Checkbox |
|---|---|---|
| `## Backlog` | not scheduled | `- [ ] ` |
| `## Bugs` | defects, any priority — the column **is** the marker, so **no `🐛` prefix** | `- [ ] ` |
| `## Specification` | clear outcomes, scope boundaries, constraints and **verification criteria**, before any code or text is written | `- [ ] ` |
| `## Generation` | build the raw solution **strictly to the defined contract** | `- [ ] ` |
| `## Validation` | run tests, linters and logic checks against the output **to isolate specific failures** | `- [ ] ` |
| `## Revision` | fix **only what failed**, preventing unguided drift or wholesale rewriting | `- [ ] ` |
| `## Audit` | log final assumptions, behaviour consistency and security boundaries for human review | `- [ ] ` |
| `## Done` | shipped and verified — goes **below the `**Complete**` line** | `- [x] ` |

`Backlog` and `Bugs` are intake lanes, not stages: a card enters the pipeline at `Specification`.

Three rules carry the weight:

**Specification writes the verification criteria.** They are the contract. Validation does not
invent its own standard later — it runs what Specification already committed to. A plan with no
verification criteria is not specified, however detailed it looks.

**Validation isolates failures; it does not fix them.** Its output is *which* check failed and on
what input. Fixing inside Validation is how a card silently becomes a rewrite.

**Revision touches only what failed.** Not the neighbouring function, not the naming, not the
thing that looked wrong on the way past. Unguided drift is the specific thing this stage forbids.
Anything else found goes on a new card.

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

Read the first word of the argument; infer from the conversation if absent. The stage words are
the column names — moving a card is naming what has just been finished.

| Arg | Action |
|-----|--------|
| `plan` / `spec` | Draft a codebase-grounded plan note → `docs/plans/<slug>.md`, then add a card to `## Specification`. |
| `implement` / `start` | Move the card to `## Generation`; ensure its plan note exists and is current. |
| `validate` | Move to `## Validation`. **Then actually validate it** — see below. |
| `revise` | Move to `## Revision` (validation found something). |
| `audit` | Move to `## Audit`. |
| `done` | Move to `## Done` below `**Complete**`, and flip `- [ ]` → `- [x]`. |

### What each stage owes

- **Specification** — outcomes, scope boundaries, constraints, and the **verification criteria**:
  the concrete checks that will decide whether this is done. Named modules, tables and files;
  open decisions stated; what is out of scope. Written before any code.
- **Generation** — the solution built to that contract, and nothing beyond it. A good idea that is
  not in the spec is a new card, not a bonus.
- **Validation** — run the criteria. Report **which** failed and on what input, precisely enough
  to act on. In this project that means proving it works *where it has to work*: for the web app
  the **served bundle**, not a green pipeline; for a device, the **device**; for a migration, a
  real Postgres. This project has repeatedly shipped changes that compiled, type-checked, deployed
  and did not work — "it built" is not a criterion.
- **Revision** — fix exactly those failures, then re-run Validation. Note on the card what failed
  and what changed. A card bouncing Validation → Revision → Validation is the pipeline working.
- **Audit** — write down what the work assumed, whether behaviour stayed consistent with what the
  docs claim, and where the security boundaries ended up. Update the reference docs, resolve the
  plan note's open decisions, and confirm no credential or secret landed anywhere it should not.

### Move a card
Read the file, then cut the card's lines — the `- [ ]` line **and** its indented `Plan:` line if it
has one — from the current column and paste them into the target column. Two rules:

- **Keep the pair together.** An orphaned `Plan:` line renders as a stray card.
- **Only `## Done` flips the checkbox.** Every other column keeps `- [ ]`.

Then confirm in one line, e.g. "Moved #119 → Validation."

### Plan a card
1. **Find + read the use case** in `docs/use-cases/` (fuzzy-match the name).
2. **Ground the plan in the codebase** (Glob/Grep/Read) — name real modules/crates/nodes/tables,
   reuse existing infra, honour project memory.
3. **Write the plan** to `docs/plans/<slug>.md` (`<slug>` kebab-case): phases (ordered, buildable
   slices), open decisions, out-of-scope.
4. **Add the card** to `## Specification` with the next `#N` and a `Plan: [[<slug>]]` line.
5. Several clearly-independent features → one card each, each with its own note.
6. For `implement` with **no** existing card → create one directly in `## Generation`, and write
   the plan note anyway. Skipping Specification because the work already started is how a card ends
   up with no record of what it was meant to do.

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
## Verification criteria
- the concrete checks that decide this is done, written BEFORE the code
- each one testable by someone who did not write it
## Open decisions
1. …
## Out of scope (v1)
- …
```

**The verification criteria are the contract Validation runs.** Write them in Specification, where
they are a design decision, not afterwards, where they become a description of whatever got built.
Prefer criteria that name a place and an observation — "the served bundle contains X", "the device
answers Y", "the migration applies over the full chain on a real Postgres" — over "works".

`[[wikilinks]]` are fine now that the board is Obsidian. They do **not** render on GitHub, so use
plain repo-relative links for anything a non-Obsidian reader needs to follow.

## Commit the change

`docs/Task Board.md` is a tracked file, so a board move is a commit like any other — commit it
(plus any new plan note) rather than leaving it dirty in the working tree.

**The vault is local.** Committing is what makes a board change visible to anyone else; if a
collaborator needs to see the state, the commit is not optional. This is the one real cost of
moving off the shared GitHub Project — worth saying out loud when board state matters to someone
other than the operator.
