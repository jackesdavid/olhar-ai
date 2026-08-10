# What speaks the Tuya LAN protocol from the edge

Type: research
Status: open

## Question

The edge is Rust and deliberately pure-Rust (no OpenCV, no sidecars). Establish what implements the
Tuya LAN protocol there.

- Is there a maintained Rust crate covering protocol versions **3.1 through 3.5**, or is the honest
  answer that we port it? tinytuya's `PROTOCOL.md` documents the wire format, so porting is bounded
  work rather than reverse engineering — but it is still work, and knowing which it is changes the
  estimate.
- What each version actually requires. 3.4 and 3.5 introduced a session handshake rather than the
  simple AES-ECB of 3.1/3.3; supporting only the old ones would quietly exclude newer hardware.
- Whether one TCP connection per device is held open or dialled per command. The edge already has a
  hard rule about not opening a second connection to a camera; a device's tolerance for concurrent
  connections deserves the same question before it is discovered in the field.
- What discovery on UDP 6666/6667 yields: does the broadcast carry the protocol version and device
  id, or only enough to know something is there?

AFK. The answer decides whether this is "add a crate" or "port a protocol", which is the difference
between days and weeks.
