# What the approval screen looks and feels like

Type: prototype
Status: open
Blocked by: 01, 02

## Question

Make something rough and concrete to react to, rather than argue about it in prose.

Must show: the flow name (small), the message (large), a countdown to the TTL, slide-to-approve, and
an ordinary Deny button. Must also render honestly the states that are not "answer me": already
answered, expired, request unknown, and no connectivity.

Open questions the prototype should force answers to:

- How long is the slide, and does it snap back or hold? Long enough that it cannot mis-fire, short
  enough that someone in a hurry does not fight it.
- Where does the biometric prompt appear relative to the slide completing?
- What does a person see when they open the app with NO pending request — nothing, or a history?
- What does the deny path ask, if anything? A confirmation on deny is friction on the safe
  direction; none at all risks a mis-tap denying something.

Blocked by [Who may answer](./01-who-may-answer.md) and
[Biometrics on a device that has none](./02-biometrics-without-biometrics.md): both change what the
screen must handle, and prototyping before them means prototyping twice.
