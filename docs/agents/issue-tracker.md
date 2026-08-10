# Issue tracker: the Obsidian board

Work is tracked in **`docs/Task Board.md`** — an Obsidian Kanban file, committed to the repo. It is
NOT GitHub Issues and NOT a GitLab issue tracker; see [[board-is-obsidian]]. Detail lives beside it
in `docs/plans/<slug>.md`, linked from the card with `[[wiki-links]]`.

This split is the thing to understand before anything below makes sense:

- **The board card is the index entry** — one line, a number, a title, a link to the plan.
- **The plan note is the detail** — reasoning, measurements, decisions, what was tried.

That is exactly the map/ticket split `/wayfinder` describes, which is why it maps cleanly.

## Conventions

- Cards live under a `## <Column>` heading — `Backlog`, `In Progress`, `Review`, `Done`.
- A card is `- [ ] #<n> <title>`, with indented sub-lines for the plan link and a short gist.
- Numbers are allocated by taking the highest `#<n>` on the board and adding one. **Read the board
  first** — a duplicated number makes two pieces of work indistinguishable in every later reference.
- Moving a card between columns means moving the lines under a different heading. There is no state
  field.
- Plan notes are `docs/plans/<slug>.md` and are referenced from the card as `[[slug]]`.

## When a skill says "publish to the issue tracker"

Add a card to `## Backlog` in `docs/Task Board.md` and write its detail to `docs/plans/<slug>.md`.

## When a skill says "fetch the relevant ticket"

Read the card's lines on the board, then read the plan note it links to. The card alone is a title;
the reasoning is in the note.

## Wayfinding operations

Used by `/wayfinder`. The skill assumes a tracker with child issues, native blocking and assignees.
This board has none of those, so each is expressed with what it does have — a committed markdown
file and a directory of notes.

- **Map**: `docs/plans/<effort>-map.md`, with the Destination / Notes / Decisions-so-far /
  Not-yet-specified / Out-of-scope body. Its board card carries `[map]` in the title so it is
  distinguishable at a glance from ordinary work.
- **Child ticket**: `docs/plans/<effort>-map/NN-<slug>.md`, numbered from `01`. A `Type:` line
  records `research`/`prototype`/`grilling`/`task`; a `Status:` line records
  `open`/`claimed`/`resolved`.

  Tickets are files in a directory rather than board cards ON PURPOSE. A map can carry a dozen
  open questions, and putting each on the board would bury the actual work — the board is the
  human's view of what is being built, not of what is being decided.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it
  names is `resolved`.
- **Frontier**: the files in the map's directory that are `open`, unblocked and unclaimed; lowest
  number first.
- **Claim**: set `Status: claimed` and SAVE before any work.

  There are no assignees here, so the claim is a line in a file. It only works if the file is saved
  before the work starts — the whole point is that a concurrent session reading the directory skips
  it. Claiming after the fact claims nothing.
- **Resolve**: append the answer under `## Answer`, set `Status: resolved`, then append a one-line
  gist plus link to the map's Decisions-so-far.

### Referring to tickets

`/wayfinder` says to refer to everything by name, never by bare number. That holds here and matters
more, because a filename like `03-storage-shape.md` is not a name — write **"Storage shape"** and
carry the path as the link.

### Concurrency

The tracker is a file in git, not a server. Two sessions editing the board or one map's directory
will conflict as ordinary merge conflicts, and a `Status: claimed` written but not committed is
invisible to everyone else. Prefer one wayfinder session at a time; when running several, keep them
on separate maps.
