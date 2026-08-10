# Who may answer a gate request

Type: grilling
Status: resolved

## Question

A request is addressed to one Recipient — a specific phone. Who is actually allowed to settle it?

The candidates are not equivalent:

- **Only the addressed device.** Strictest, and matches what the graph author wrote. But a phone
  that is dead, lost, or replaced makes that gate unanswerable until someone edits the flow.
- **Any device belonging to the addressed user.** Survives a phone swap. Requires the request to be
  addressed to a person, not a device — and today a device IS a Recipient (#139), so this changes
  what a Recipient means.
- **Anyone in the tenant holding a permission.** Most operationally forgiving, and the weakest: a
  gate the author meant for one person becomes answerable by whoever holds the role.

This is the sharpest risk on the map because it decides the DATA MODEL — whether the request points
at a device, a user, or a permission — and everything downstream (the pending list, the answer
endpoint's authorization, the audit line) inherits that choice.

Resolve with: what does the customer actually expect when the addressed phone is flat at 2am?

## Answer: only the addressed device

The request points at a `push_devices` row and nothing else may settle it. **Recipient keeps the
meaning it has today** (#139) — a device IS a Recipient — so nothing about the picker, the
notification path or the existing model changes.

Chosen knowing the cost: a phone that is flat, lost or replaced makes that gate unanswerable until
someone edits the flow. That is the honest trade for the strongest guarantee this node can make —
the person the author addressed is the person who answered, and the audit line says so without
qualification.

The 2am objection is real but it is answered elsewhere rather than by widening who may answer.
A gate that nobody can open is a safe failure; a gate that the wrong person can open is the
incident this node exists to prevent. Expiry takes Failure and the graph decides what happens next.

**What this fixes downstream:**

- `device_gate_requests.push_device_id` → `push_devices(id)`. Not a user id, not a permission.
- The answer endpoint authorizes on THREE things: a valid session, the session's user owning that
  push device, and the token matching. No new RBAC permission is added — a permission-based path is
  precisely what this decision rejected.
- `GET /device-gate/pending` is scoped to the calling device, not the user's devices.

**Interaction with [02](./02-biometrics-without-biometrics.md), which decided it:** if only one
device may answer, refusing approval on a phone without enrolled biometrics would make that gate
permanently unanswerable. 02's passcode fallback is what keeps this decision survivable — the two
answers are load-bearing on each other and should not be revisited separately.
