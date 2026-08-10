# How the per-execution token is minted, stored and compared

Type: research
Status: open

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
