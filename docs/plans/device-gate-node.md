# Device Gate — a human approves, from their phone, before the graph continues

**Goal:** a workflow node that stops and asks a person. It pushes an authorization request to the
phone, the person slides to approve (confirming with biometrics) or denies, and the graph continues
down **Positive**, **Negative** or **Failure** accordingly.

**Driving case:** opening an electric gate through a Tuya device. Opening a gate is not a switch —
it is a security decision, and it should not share a node with turning on a lamp.

**Agnostic by design.** The node knows nothing about Tuya, gates or locks. It gates an *exec edge*;
whatever hangs off Positive is the sensitive action, whether that is a Tuya control, an HTTP call, or
a door relay we do not support yet.

## Shape

```
        exec in ──►  Device Gate  ──► Positive  (a person approved)
                                  ├─► Negative  (a person denied)
                                  └─► Failure   (timed out, undeliverable, error)
```

### Properties panel

| Field | What it is |
|---|---|
| **Recipient** | which device is asked |
| **Message** | what the person reads — the whole basis for their decision |
| **Timeout (TTL)** | how long the request stays answerable before it takes Failure |

The **flow name is NOT a field.** It is taken from the graph and sent with every request, shown
small on the approval screen. Making it typed would let the text disagree with the flow actually
asking — and "which automation wants this?" is a question whose answer must not be authorable by
whoever writes the prompt. Small, but present: a person approving a gate should never have to
recognise a request purely from its wording.

The message deserves care in the UI. It is the only thing standing between a push notification and
someone opening a gate, so the field should be a multi-line input with a visible hint that it is
what the person will read on their phone — not a one-line box that invites "ok?".

`Failure` on timeout, not `Negative`: the TTL expiring means nobody decided.

### Why three arms and not two

An explicit denial is an ANSWER, and it is the most informative thing this node produces: someone
was there, read the request, and said no. Folding it into a generic failure throws that away and
leaves the graph unable to tell it from a phone that was face-down on a table.

The two also want opposite handling. A denial is usually terminal — log it, notify, stop. A timeout
often wants a retry, an escalation to a second recipient, or a fallback. A single arm forces every
graph to re-derive which happened from something else, and the graphs that skip that step are the
ones that treat "nobody answered" as "go ahead", which is the incident this node exists to prevent.

`Failure` stays deliberately broad — timeout, no registered device, push rejected, control
unreachable. What they share is that **no human decided**, and that is the property a graph branches
on.

## The one hard constraint

**A run cannot block waiting for a person.** `window_consumer` already states why, for
`DelayTrigger`:

> a node's handler runs to completion before the engine settles it, so a `DelayTrigger` that
> blocked would also hold back its own `start` branch. The wait is a row in Postgres instead.

Device Gate is the same shape, and should reuse the same machinery rather than invent a second
suspend mechanism:

1. The node fires the push and **returns without taking any branch**. The run ends.
2. A row holds the pending request — durable, so an armed gate survives a pod restart.
3. The answer (or the timeout) **resumes at that node** via `spawn_node_subrun`, with a payload
   carrying the verdict; the node reads it and fires Positive, Negative or Failure.

`spawn_node_subrun` is already parameterised by trigger source and payload (done for the cron node),
so this needs no new engine primitive — which is the strongest argument for this design over any
other.

## What already exists

| Piece | Where | State |
|---|---|---|
| Push tokens per device, **a device IS a Recipient** (#139) | `push_devices`, `notification/push.rs` | done |
| Recipient picker in the editor | `node/recipients.rs` | done |
| Durable suspend + resume-at-node | `work_jobs`, `spawn_node_subrun` | done |
| Deep link scheme `olharai://` | `app.json` | declared |
| `expo-notifications` | mobile deps | installed |

## What has to be built

**Backend**
- A `device_gate_requests` table: request id, tenant, graph, node, run, recipient, prompt, state
  (`pending`/`approved`/`denied`/`expired`/`failed`), who answered, when, and the approval evidence.
  The four settled states map onto three arms — `approved` → Positive, `denied` → Negative,
  `expired`/`failed` → Failure — and are stored SEPARATELY rather than as the arm taken, because the
  audit trail needs to say which one happened.
- The node: push on entry, return without a branch.
- `POST /device-gate/:id/answer` — the phone's verdict. Must be **idempotent** and must reject a
  second answer to a settled request, or a retried tap could approve something twice.
- A consumer that expires a pending request at its deadline and resumes down Failure.

**Mobile**
- Handle the notification tap → deep link to the approval screen (no listener today).
- The approval screen: the node's description, slide-to-approve, deny.
- Biometrics on APPROVE only (`expo-local-authentication`, not currently a dependency).
- Maximum-attention delivery: sound, vibration, a critical/time-sensitive category.

## Decisions that need making, not assuming

1. **Who may answer?** The addressed Recipient only, or anyone in the tenant with a permission? A
   gate approved by the wrong person is the failure mode this node exists to prevent.

2. **Is biometry required or best-effort?** A phone with no enrolled biometric still needs to work,
   but "approve anyway" quietly turns the strongest control into a suggestion. Refuse on such a
   device, or fall back to the device passcode?

3. **Replay and reuse.** The request id must be single-use, and the notification carries it. If the
   same push is delivered twice — which happens — the second tap must land on a settled request and
   say so, not open a second slider.

4. **What the phone shows when it cannot reach control.** Approving offline and syncing later is
   exactly wrong for a security gate; the screen should fail closed and say why.

## Security notes worth stating up front

### One secret per execution, and the answer must carry it back

Every entry into this node mints its OWN unguessable token. The push carries it, and an answer is
accepted only if it returns the same one. No token, wrong token, or a token for a different
execution — refused.

This is what makes the node safe to expose at all. Without it the endpoint is `POST
/device-gate/:id/answer` with a guessable or enumerable id, and anyone who can reach control can
approve someone else's gate. The token is the proof that the answer came from the phone that was
actually asked.

Three properties it must have, and each one has a real failure behind it:

- **Per execution, never per node or per graph.** A token that outlived one request would let a
  replay of yesterday's approval open the gate today.
- **Single use.** Settling a request burns its token, so a duplicate push delivery — which happens —
  lands on a settled request and says so instead of opening a second slider.
- **Compared in constant time, and never logged.** A token that reaches a log file is a token that
  reaches whoever reads logs, and this one opens a gate.

The token is necessary but NOT sufficient: the answer must still come from an authenticated session
whose user is allowed to answer that request. The token proves *which request*; the session proves
*who*. Neither alone is enough — a leaked token must not be usable by a stranger, and a valid
session must not be able to answer a request it was never sent.
- The prompt is authored in the graph, so it is **operator-controlled text rendered on a phone**.
  Treat it as untrusted for display purposes.
- Every answer belongs in the audit trail (`events`), with who, when, and from which device. This is
  the node whose history someone will eventually need to read back.

## Test shape

The property with no other guard: **a settled request cannot be settled again.** Two taps, one
approval. Worth an explicit test, because the failure is silent and the consequence is a gate that
opens twice.

Then the end-to-end: a graph whose Positive arm flips a Tuya device, approved from a real phone —
and a denial on the same graph reaching Negative, not Failure.

## Related
[[tuya-integration]] · [[workflow-engine-loops-events]] · [[mobile-edge-face-capture]]
