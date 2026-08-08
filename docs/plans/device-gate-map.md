# Device Gate [map]

## Destination

Every decision the Device Gate node needs, locked — so someone can build it without stopping to
ask. Not the built node: the point at which nothing is left to decide.

## Notes

**Domain:** a workflow node that suspends a run, asks a person on their phone to authorize a
sensitive action, and continues down Positive / Negative / Failure. Agnostic — it gates an exec
edge, not a Tuya device.

**The settled baseline is [[device-gate-node]].** Read it before taking a ticket; it holds the
shape, the three arms and why, the per-execution token, the properties panel, and the mobile
experience. This map exists for what that note leaves open, not to restate it.

Already fixed, do not reopen:
- Three exec arms. A denial is an ANSWER (Negative); Failure means no human decided.
- One unguessable token per EXECUTION, returned with the answer. Single use, never logged.
- The token proves WHICH request; the authenticated session proves WHO. Neither alone.
- Suspension is a durable row + `spawn_node_subrun`, not a blocked handler.
- Panel: Recipient, message, TTL. The flow name is sent automatically, never typed.
- The node shows a live, decaying countdown on the canvas while a request is pending — same
  treatment the cron node got.

**Skills:** `/grilling` and `/domain-modeling` on every conversation ticket. `/research` runs AFK.

**Standing preference:** decisions, not deliverables. The pull to start building is the signal the
map is done.

## Decisions so far

<!-- one line per resolved ticket -->

## Not yet specified

- **Abuse and rate limiting.** Nothing stops a graph from firing approval requests in a loop. Where
  the limit belongs — the node, the recipient, the tenant — depends on how the run behaves while
  suspended, so it cannot be phrased sharply yet.
- **A pending request whose graph changed underneath it.** The node deleted, the graph disabled, the
  TTL edited, the Recipient swapped. Each is a different answer and they hang on the run-state
  decision.
- **Several requests waiting for the same person at once.** Whether they queue, stack, or collapse
  is a product question that only gets sharp once the phone experience is prototyped.

## Out of scope

- **Escalation to a second Recipient after a timeout** — ruled out deliberately. Expiry takes
  Failure and the graph decides what happens next; putting a retry ladder inside the node hides the
  policy from the person who writes the flow.
- **A dedicated approvals screen in the web app** — every answer lands on the product event stream
  like everything else. A bespoke history page can come later if the events prove insufficient.
