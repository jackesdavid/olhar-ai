# What the run IS while a request is pending

Type: grilling
Status: open
Blocked by: 01

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
