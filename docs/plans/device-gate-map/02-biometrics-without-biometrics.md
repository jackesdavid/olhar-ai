# Biometrics on a device that has none

Type: grilling
Status: resolved

## Question

Approval requires a biometric confirmation. What happens on a phone with no enrolled fingerprint or
face — an older device, a work phone with it disabled by policy, a user who simply never set it up?

- **Refuse.** That device can never approve. Honest, and it makes the guarantee real: an approval
  always carried a biometric. Also means a customer can be locked out by their own phone settings.
- **Fall back to the device passcode.** Everyone can approve. But "approve anyway" quietly turns the
  strongest control into a suggestion, and nothing in the audit trail would say which one was used —
  unless we record it, which is itself part of this decision.

Note the interaction with [Who may answer](./01-who-may-answer.md): if only one device may answer
and that device cannot do biometrics, refusing means the gate is unanswerable.

Resolve with: is a biometric a REQUIREMENT of the product or a property of the device? And whatever
the answer, the evidence recorded with the approval must say which factor was used.

## Answer: fall back to the device passcode, and record which factor was used

A biometric is a property of the DEVICE, not a requirement of the product. `expo-local-authentication`
prompts with whatever the phone has; if no biometric is enrolled it falls back to the passcode, and
that is accepted.

What stops this from quietly turning the strongest control into a suggestion is the second half,
which is not optional: **the approval evidence records the factor.** `device_gate_requests` carries
an `auth_factor` column (`biometric` | `passcode`), it goes on the `events` audit line with the
answer, and it is readable later. An approval whose strength is unknown is the thing being avoided
here — not the passcode itself.

Rejected: refusing outright. Combined with [01](./01-who-may-answer.md)'s "only the addressed
device", refusing would mean a customer can lock themselves out of their own gate through a phone
setting they may not control — a work phone with biometrics disabled by policy is a real
configuration, not a hypothetical.

Also rejected: falling back silently. Same behaviour, no evidence, and nothing in the history to
say what happened. It is the cheapest to build and the only option with no way back if it turns out
to matter.

**A device with no passcode at all** cannot authenticate and cannot approve. That is not a policy
we are choosing — the OS has nothing to prompt with — and the screen should say so plainly rather
than appearing to work.
