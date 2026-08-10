# What the run IS while a request is pending

Type: grilling
Status: resolved

## Question

The node suspends and the answer resumes it. But what does the RUN look like meanwhile — and what
does the rest of the engine believe?

Two shapes, and they differ in more than presentation:

- **The run ends; the answer starts a new one.** Exactly what `DelayTrigger` does today, so it needs
  no engine change. The run history then shows two entries with no obvious link, and whoever audits
  an incident has to join them by hand.
- **The run stays open in a `waiting` state.** Reads honestly and keeps the story in one place. But
  it touches concurrency: a graph on `SkipIfRunning` would refuse every new trigger for as long as
  someone leaves their phone face-down, which turns a missed notification into a silently dead
  automation.

Already decided: the NODE shows a decaying countdown on the canvas while pending. That is a
presentation commitment, and it is satisfiable either way — but the two shapes make it easy or hard,
so the decision should be made knowing which.

Blocked by [Who may answer](./01-who-may-answer.md): if a request can outlive the addressed device
and wait for another, the pending window is potentially much longer, which sharpens the concurrency
question considerably.

Resolve with: what a `SkipIfRunning` gate graph should do when a second trigger arrives while a
request is pending.

## Answer: the run ENDS; the answer starts a new one

The baseline already committed to this — *"Suspension is a durable row + `spawn_node_subrun`, not a
blocked handler"* — and unblocking on [01](./01-who-may-answer.md) confirms it rather than
reopening it. Writing it down because the ticket asked what the rest of the engine believes, and
"nothing, the run is over" is a real answer with consequences.

`DelayTrigger` does exactly this today, so there is no engine change and no second suspend
mechanism to keep correct.

**The `SkipIfRunning` case settles it.** With the run open in a `waiting` state, a graph on
`SkipIfRunning` would refuse every new trigger for as long as someone leaves their phone
face-down — a missed notification becoming a silently dead automation. That is a worse failure than
the one the alternative causes, and it is invisible while it happens. With the run ended, a second
trigger runs normally and mints its own request; two pending requests for the same gate is a
legible situation, and [05](./05-attention-limits.md)'s finding that pushes cannot be relied on to
pierce Focus makes "a second trigger still works" the behaviour we want.

**The cost, stated:** run history shows two entries with no obvious link, and whoever audits an
incident joins them by hand. Mitigated rather than accepted — the `events` audit line for the
answer carries the originating run id, so the join exists in the data even though the run list does
not draw it. That is enough for an audit and short of a UI change this ticket is not buying.

The canvas countdown is satisfiable either way and stays as decided: the node reads its pending
request row, not the run state.
