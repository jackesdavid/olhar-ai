# What speaks the Tuya LAN protocol from the edge

Type: research
Status: resolved

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

## Answer

**It is "add a crate".** `rustuya` covers 3.1 through 3.5, is MIT, and is pure Rust with no C
dependency. Days, not weeks — but the crate is young, and that caveat is the real finding.

### The crate

[`rustuya`](https://github.com/3735943886/rustuya) 0.3.0, MIT, released 2026-07-04. Its README
claims, in as many words, *"Full protocol coverage — Tuya 3.1 / 3.2 / 3.3 / 3.4 / 3.5 + device22"*,
and its dependency list backs the claim rather than merely asserting it: `aes` + `ecb` for the old
versions, `aes-gcm` for 3.5, `hmac` + `sha2` for the 3.4 handshake and its HMAC-SHA256 integrity
field, `md-5` for the 3.1 signature and the discovery key. The protocol layer is derived from
tinytuya's `PROTOCOL.md` — the same document we would have ported from.

It fits the edge's constraints exactly:

| Constraint | rustuya |
|---|---|
| Pure Rust, no C / no OpenCV / no sidecar | RustCrypto + tokio + `socket2`. No `-sys` crate, no OpenSSL |
| No Python in the edge | Rust crate is `crate-type = ["rlib"]`; the PyO3 bindings are a separate artefact |
| Builds on `rust:1-bookworm` | needs edition 2024 / rustc 1.88; CI tracks stable, and the edge stays edition 2021 — a dependency's edition is its own |
| Async on tokio | tokio 1.49, same runtime the edge already runs |

**The caveats, stated plainly.** 1,496 downloads total, first release 2025-12-30, one maintainer.
The fleet-scale testing is real but is against the author's own `tuyamock` — 1000 mock devices with
actual TCP connections and handshakes, which proves the concurrency machinery and proves nothing
about protocol quirks in real hardware. Version churn is high (ten 0.3.0 release candidates between
May and July). This is a crate to vendor-or-fork if it goes quiet, not one to treat as
infrastructure. It is still far cheaper than porting five protocol versions, and the fallback if it
dies is the port we were going to do anyway.

The alternatives are worse and were checked: `tuya-rs` is v3.3 only with 171 downloads;
`rust-tuyapi` / `rust-async-tuyapi` are ports of the NodeJS **cloud** client, 3.1/3.3, not this job.

### What each version requires

Two frame formats, not one — 3.5 changes the envelope, not just the cipher:

- **`55AA`** (3.1–3.4): `000055AA | seq | cmd | len | payload | integrity | 0000AA55`
- **`6699`** (3.5): `00006699 | reserved | seq | cmd | len | IV(12) | ciphertext | tag(16) | 00009966`

| | 3.1 | 3.2 | 3.3 | 3.4 | 3.5 |
|---|---|---|---|---|---|
| Cipher | AES-ECB (control only) + base64 + MD5 | AES-ECB | AES-ECB | AES-ECB, **session key** | AES-GCM, **session key** |
| Integrity | CRC32 | CRC32 | CRC32 | HMAC-SHA256 | GCM tag |
| Handshake | none | none | none | 3-way | 3-way |

The 3.4/3.5 handshake is three messages encrypted with the real `local_key`: client nonce →
device nonce + `HMAC-SHA256(client_nonce, key)` → `HMAC-SHA256(device_nonce, key)`. Both sides then
derive `session_key = AES(key, client_nonce XOR device_nonce)`.

The practical consequence for us: **the handshake is per-connection.** A session key is negotiated
on connect and dies with the socket, so on 3.4/3.5 a dial-per-command design pays three extra
round-trips on every command. That is an argument for the persistent connection below, not just a
performance note.

### Connections: one per device, held open — and the device enforces it

This is the finding that matters most, because it is the same shape as a rule the edge already has.

> "Tuya devices only allow one TCP connection at a time. Make sure you close the TuyaSmart or
> SmartLife app before using *TinyTuya* to connect." — tinytuya README

**This is the camera ONE-PIPELINE rule again, on different hardware.** `capture.rs` caps a consumer
camera at one pipeline because a second RTSP session is how a working camera goes dark; a Tuya
device has the same intolerance, and the failure will look the same — a device that "just stopped
responding" with nothing in the logs to say why. Whatever holds these connections needs the
invariant written down where someone will trip over it, exactly as `capture.rs` does.

The device also closes an idle socket at about 30 seconds, so holding the connection means sending
a heartbeat every 8–10s. rustuya already does this: a per-device background task, heartbeats,
automatic reconnection with exponential backoff, and a bounded connect-concurrency cap so a fleet
coming up at once does not deadlock.

**Consequence for the product, not just the code:** if the edge holds the only local connection,
the tenant's own Smart Life app cannot take it. In normal use the app talks to Tuya's cloud rather
than the LAN, so contention should not appear — but "should not" is doing work in that sentence,
and this is the sort of thing a customer reports as "your box broke my app". It belongs in ticket
03's list of devices and situations that misbehave, and it wants a real-hardware check before we
promise anything.

### Discovery

Broadcasts are unsolicited and carry `gwId` (device id), `ip`, and the protocol `version` — enough
to build the whole map without a scan, so the edge learns a device's version rather than being told
it.

| Port | Versions | Encryption |
|---|---|---|
| 6666 | 3.1 | plaintext JSON |
| 6667 | 3.2–3.4 | AES-ECB under the well-known key `MD5("yGAdlopoPVldABfn")` |
| 7000 | 3.5, newer hardware | client sends `{"from":"app","ip":...}`, device unicasts a 6699/GCM reply |

Note the shape change at 7000: it is **solicited**, so a passive listener on 6666/6667 alone will
silently miss the newest devices. No key material is ever broadcast — the `local_key` still has to
come from the Cloud API, which is ticket 02's problem and is not softened by any of this.

## What this decides, and what it hands on

Locked: **`rustuya`, one persistent heartbeated connection per device, discovery on all three
ports.** No port, no protocol work, no new language in the edge.

Handed to other tickets:

- **→ 02 (key delivery):** the `local_key` **changes every time a device is removed and re-added, or
  reset and re-paired**, in the app. So key delivery is not one-shot at onboarding; there is a
  rotation path whether or not we design one, and a stale key fails the 3.4/3.5 handshake rather
  than failing a command in a way anyone can read.
- **→ 03 (devices that cannot go local):** the app-contention question above, plus the fact that
  the widely reported failure in this ecosystem is not "wrong protocol" but "local connection
  stuck, never recovered". Reconnection is where these integrations actually break.
- **→ 04 (command path):** one persistent connection per device means the edge holds device state
  across commands. Whatever `run_tuya_control` becomes talks to a long-lived per-device task, not a
  connect-send-close function.

## Sources

- [tinytuya PROTOCOL.md](https://github.com/jasonacox/tinytuya/blob/master/PROTOCOL.md) — wire format, version matrix, handshake, discovery
- [tinytuya README](https://github.com/jasonacox/tinytuya) — one connection at a time, persistent sockets, local_key rotation
- [rustuya](https://github.com/3735943886/rustuya) · [crates.io](https://crates.io/crates/rustuya) · [docs.rs](https://docs.rs/rustuya)
- Rejected: [tuya-rs](https://crates.io/crates/tuya-rs) (3.3 only) · [rust-async-tuyapi](https://crates.io/crates/rust-async-tuyapi) (cloud client)
