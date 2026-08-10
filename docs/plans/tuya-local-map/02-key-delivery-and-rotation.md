# How the local key reaches the edge, and what happens when it changes

Type: grilling
Status: open

## Question

`local_key` is a secret and must live on the edge, never in the cloud DB — the same rule camera
credentials follow. Two halves to settle.

**Delivery.** Camera credentials use a one-shot, memory-only staging channel: control holds them
until the edge collects them on its next config pull, then discards. The same channel is the obvious
home for local keys, but they arrive differently — from a cloud SYNC rather than from a person
typing — so who triggers the hand-over, and when, is the question.

**Rotation, which is the harder half.** The key changes when a device is re-paired. A stored key
that is silently wrong makes a device that no longer responds, with nothing saying why — the same
failure shape as several bugs already hit this month. So:

- How does the edge tell "wrong key" from "device offline"? The protocol's failure mode on a bad key
  decides whether this is detectable at all.
- Who re-fetches — the edge asking control to re-sync, or control noticing on its own?
- What does the user see meanwhile? A gate that stopped working needs to say "re-pair detected,
  refreshing" rather than nothing.

Resolve with: what should happen the first time someone re-pairs a plug in the Smart Life app.

## What 01 established

[01](./01-protocol-client-in-rust.md) answered the detectability question, which was the one that
decided whether the rest is designable:

- **A wrong key is distinguishable from an offline device.** On 3.4/3.5 the key is used in a session
  handshake before any command — a bad key fails the handshake, a dead device fails to connect.
  Two different failures at two different stages, so the edge can say which. On 3.1–3.3 there is no
  handshake and a bad key surfaces as a garbage decrypt instead; still distinguishable from a
  refused connection, but by a different signal, and the UI copy has to hold for both.
- **Rotation is not a rare event to design around later.** tinytuya states it outright: the key
  changes every time a device is removed and re-added, or reset and re-paired. Any tenant who
  troubleshoots a misbehaving plug the ordinary way — delete it, pair it again — rotates the key.
- The `local_key` is never broadcast on the LAN. Discovery gives id, ip and version only, so the
  Cloud API remains the sole source and there is no local shortcut that would avoid this ticket.
