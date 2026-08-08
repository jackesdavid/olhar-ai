# Biometrics on a device that has none

Type: grilling
Status: open

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
