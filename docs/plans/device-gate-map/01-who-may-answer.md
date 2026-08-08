# Who may answer a gate request

Type: grilling
Status: open

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
