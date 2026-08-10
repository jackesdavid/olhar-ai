# How the per-execution token is minted, stored and compared

Type: research
Status: resolved

## Question

The design is fixed: one unguessable token per execution, returned with the answer, single use,
never logged. What is NOT fixed is how to build it — and this repo already has several token
patterns that should not become a fifth.

Survey what exists and report the shape to copy:

- Box tokens (`new_box_token`, `hash_token` in `sentinel-control`) — how are they generated, and are
  they stored hashed?
- Password reset tokens (migration 0087) — expiry, single use, how consumption is enforced.
- Edge request signing (`signer.rs`, per-box HMAC keys) — is a signature a better fit here than a
  bearer token?

Then answer: generation (source of randomness, length), storage (plaintext or hash), comparison
(constant time — what does the codebase use today), and how single-use is enforced against a
concurrent double-tap. That last one is a race, not a policy: two taps arriving together must not
both settle the request.

AFK. Findings go in the answer; no decision is being asked of a human here beyond picking the
existing pattern that fits.

## Answer: copy the password-reset token, but NOT its settle path

### The pattern to copy

`sentinel-control/src/password_reset.rs` is the closest fit and is already the newest of the three:

- **Generation:** 32 bytes from `rand::thread_rng().fill_bytes()`, hex-encoded — 256 bits. Guessing
  is not a strategy. (`new_box_token` instead concatenates two UUIDv4s; same CSPRNG underneath, but
  the explicit byte buffer says what it is.)
- **Storage: the SHA-256 hash only** (`hex::encode(Sha256::digest(raw))`), as migration 0087 does.
  A dump of `device_gate_requests` must not be enough to open a gate. The raw token exists in the
  push payload and in memory during the request that mints it, nowhere else.
- **Comparison: there is none, and that is the finding.** Nothing in this repo does a constant-time
  compare — no `subtle`, no `ct_eq` — because tokens are looked up BY their hash rather than
  compared byte-for-byte. A primary-key probe on a hash leaks nothing useful about the secret, so
  the baseline's "compared in constant time" is satisfied structurally by the storage decision. Do
  not add a `subtle` dependency to satisfy the wording; the shape already meets the intent.

### Where password reset must NOT be copied

Its confirm path is `SELECT` → check `used_at` → do the work → `UPDATE ... SET used_at`. That is
**check-then-act**, and two concurrent requests can both pass the check. For a password reset it is
benign: both writes set the same password. For a gate it is the exact race this ticket flagged —
two taps could both settle, and the second would resume the run a second time down an arm that was
already taken.

Settle in **one atomic statement** instead, with the state guard in the `WHERE`:

```sql
UPDATE device_gate_requests
   SET state = $2, answered_at = $3, answered_by_user = $4, auth_factor = $5
 WHERE id = $1 AND token_hash = $6 AND state = 'pending'
RETURNING id
```

Zero rows back means someone else already settled it (or the token was wrong, or it expired) — the
caller answers "already settled" and resumes nothing. One row means this caller won and is the only
one that may `spawn_node_subrun`. No window, no transaction needed, and it is what makes the
endpoint honestly idempotent.

This is a legitimate use of `RETURNING` and does not conflict with the standing rule against it:
that rule forbids reading OLD values through `RETURNING`, and this reads the post-update fact of
having won.

### Not a signature

`signer.rs`'s per-box HMAC suits a long-lived identity re-proving itself on every call. A gate
request is one-shot, minted per execution and burned on use — a bearer token in the push payload
is the right shape, and an HMAC scheme here would need key distribution to phones for no gain.
