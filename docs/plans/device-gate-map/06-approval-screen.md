# What the approval screen looks and feels like

Type: prototype
Status: resolved — built

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

## Resolved

Built rather than argued about, in `sentinel-mobile`: `app/approve.tsx` (the screen),
`src/SlideToApprove.tsx` (the gesture), `src/deviceGate.ts` (pending / answer / factor).

The open questions, answered by building them:

- **The slide runs the full width of the card and HOLDS at the end**, committing at 92% of the
  travel. Snapping back was tried in the design and rejected: a slide that springs away the moment a
  thumb lifts is a slide people fight, and the knob staying put is the visual promise that the answer
  was sent. The last 8% is where a thumb runs out of screen — demanding it turns a decisive gesture
  into a fiddly one.
- **The biometric comes AFTER the slide completes, not before.** The slide is the intent, the face is
  the proof. Prompting first would make someone authenticate merely to read the question — and on a
  request they intend to deny, that is an authentication for nothing.
- **No pending request shows "Nothing waiting", not a history.** A history here would be a second,
  worse events list; the product already has one, and this screen's job is the decision in front of
  you. Requests that are answered land on the event stream like everything else (see the map's Out
  of scope).
- **Deny asks nothing.** A confirmation is friction on the SAFE direction, and denial is an ANSWER —
  the graph learns a person said no, which is not the same as nobody deciding. The mis-tap risk is
  real but cheap: the automation simply does not happen.

Two things the build turned up that the prose had not:

- **`GET /device-gate/pending` is addressed by DEVICE, and the app was not keeping its device id.**
  `POST /push/devices` returns it and `registerThisDevice` discarded it, so the phone could receive
  an approval push and still have no way to list what it was being asked. Now stored in SecureStore.
- **The Android channel the backend stamps on every approval push did not exist app-side.** The
  backend comment said it was "created app-side at MAX importance"; it was not, so on Android the
  push arrived on an unknown channel — which Android drops. Both halves now name `device-gate` from
  a constant.

The factor is resolved by asking for biometrics ALONE first (`disableDeviceFallback: true`) and
falling back to the passcode call only if that is declined. `authenticateAsync` reports success
without saying what satisfied it, so a single permissive call would write "biometric" into an audit
trail when a passcode was typed — the exact silent downgrade decision 02 exists to prevent.
