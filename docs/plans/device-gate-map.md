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

- **Only the addressed device may answer.** The request points at a `push_devices` row; Recipient
  keeps the meaning it has today (#139). A flat phone means an unanswerable gate, accepted
  deliberately — a gate nobody can open is a safe failure. ([[01-who-may-answer]])
- **Biometrics fall back to the device passcode, and the factor is recorded.** A biometric is a
  property of the device, not a requirement of the product; `auth_factor` on the request and on the
  audit line is what keeps the fallback from being a silent downgrade. Load-bearing on 01: refusing
  would make a single-device gate permanently unanswerable. ([[02-biometrics-without-biometrics]])
- **The run ENDS while a request is pending**; the answer starts a new one via `spawn_node_subrun`,
  exactly as `DelayTrigger` does. A `waiting` run would make `SkipIfRunning` refuse every trigger
  while a phone lies face-down — a silently dead automation, the worse failure.
  ([[03-run-state-while-suspended]])
- **Token: 32 random bytes hex, SHA-256 at rest, settled in ONE atomic `UPDATE ... WHERE state =
  'pending' RETURNING id`.** Copy password-reset's minting, NOT its check-then-act settle — that is
  the double-tap race. No constant-time compare exists in this repo and none is needed: tokens are
  looked up by hash, never compared. ([[04-token-shape]])
- **The limit is a cap on UNANSWERED requests per PHONE, set per tenant, 10 by default.** Not a
  rate (asks/minute): a rate limits how fast a graph annoys someone and still lets a loop bury them
  overnight, while a depth limit bounds what a person is actually holding. Per device because
  decision 01 makes the phone the thing that can answer — per user would let two graphs bury one
  phone while the tenant total looked healthy, per graph would let ten graphs fill the same pocket.
  At the cap the node takes **Failure** (nobody decided) and says so in the run log. Expired-but-
  unswept requests do not count, or a phone that ignored yesterday would be permanently full.
  Enforced in one transaction behind a per-device advisory lock — count-then-insert is a
  check-then-act on precisely what a runaway loop does concurrently. Tenant key:
  `device_gate_max_pending` in `tenant_settings` (1–100), on Settings → General.
- **iOS `timeSensitive`, Android a MAX-importance channel — critical alerts are out of reach**
  (Apple restricts them to health/safety and declines "merely time-sensitive"). Delivery is
  best-effort, so the TTL must assume the phone is CHECKED, not alerted: `GET /device-gate/pending`
  is the reliable path and the push is the optimisation. ([[05-attention-limits]])

## Not yet specified
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
