# Box pairing — enroll by code, not by a four-secret one-liner

**Board card:** #81 Box pairing — enroll by code (ToDo)
**Goal:** A box learns who it belongs to by someone typing a short code, instead of by four secrets
being pasted onto a command line. This is what makes a double-click installer possible at all.

Unblocks: [[edge-macos-pkg]] (#82), [[edge-windows-installer]] (#83). Related: [[edge-windows]] (#80).

---

## Why (the problem the packaging question actually runs into)

Installing a box today means copying this onto the appliance:

```sh
curl -fsSL https://control…/install.sh | sudo sh -s -- \
  --control https://control… --box-token sbx_… --authkey tskey-… --hostname sentinel-1a2b3c4d
```

Four arguments, two of them secrets (`sentinel-app/src/routes/settings/boxes/+page.svelte:68`,
`sentinel-edge/install.sh:41-45`). **A `.pkg` or `.exe` that the user double-clicks has no way to
receive them.** And the obvious workaround — build a per-box installer with the token baked in —
is a dead end: a signed package cannot be modified without breaking its signature, so it would mean
signing on demand, on the VPS, with Apple's private key in reach of the server.

So the packaging work is blocked on this, not on `pkgbuild`.

## The shape

The person running the installer is **standing at the appliance** — they just double-clicked
something on it. So the installer can ask them for a code, and the edge itself can serve the page
that asks:

1. Web app → **Add box** → control mints a **pairing code** (`ABCDE-FGHIJ`) and shows it.
2. The user runs the generic, signed installer on the appliance. It ends by opening
   `http://localhost:8788` (macOS `open`, Windows `start`) — the edge's own enrollment page.
3. They type the code. The edge redeems it against control, writes `edge.env`, and restarts itself
   fully enrolled.

One artifact for everyone. No secret ever travels on a command line, and none is displayed twice.

## Security (the part to get right, not to hand-wave)

- **Nothing secret at rest.** The pairing row stores only a **hash of the code** — never the code,
  never the box token, never the edge auth key. Redeeming *mints* a fresh box token (updating
  `tenant_boxes.token_hash`, which is already how tokens are stored) and a fresh edge auth key
  (control has the OAuth client — the key-minting path (removed in #199)). So a dump of `box_pairings`
  yields nothing usable.
- **Single use, atomically.** `UPDATE … SET claimed_at = now() WHERE code_hash = $1 AND claimed_at
  IS NULL RETURNING …` — two racing redeems, one winner. (And per
  [[feedback_postgres_returning]]: this is the correct use of RETURNING, since we want the row we
  just claimed, not a pre-update read.)
- **Short-lived.** 15 minutes. A code that isn't typed in 15 minutes is a code someone left on a
  screen.
- **Unguessable + rate-limited.** 10 chars of Crockford base32 (no I/L/O/U — they get misread off a
  screen) ≈ 1.1e15 combinations. The redeem endpoint is **unauthenticated** by necessity (the box
  has no credential yet), so it gets a per-IP rate limit and returns the same generic error for
  "wrong", "expired" and "already used" — no oracle.
- **`CONTROL_URL` is not a secret** (it's a public hostname), so it can be baked into the generic
  installer. Only the token and the edge auth key are secrets, and those only ever exist in the
  redeem response.

## Phase 1 — control

- Migration **0055**: `box_pairings (code_hash text primary key, tenant_id uuid, box_id uuid
  references tenant_boxes on delete cascade, expires_at text, claimed_at text, created_at text)`.
  Timestamps are RFC3339 `text` (project convention — sqlx can't decode `timestamptz` here).
- `POST /boxes/:id/pairing-code` (auth, `devices.write`) → `{ code, expires_at }`. Additive: the
  existing `POST /boxes` (token + authkey + hostname, `domain/boxes.rs:77`) is untouched, so the
  headless one-liner keeps working.
- `POST /enroll` (**no auth**, rate-limited) `{ code }` → `{ control_url, box_token, edge_auth_key,
  hostname }`. Claims the code, rotates the box token, mints the edge auth key.

## Phase 2 — edge: unenrolled mode

- `AppState::boot()` (`sentinel-edge/src/state.rs:62`) currently assumes a `BOX_TOKEN`. When it's
  absent → **unenrolled mode**: don't pull cameras, don't start the hub/tracking, just serve HTTP.
- `GET /` → a small self-contained enrollment page (one input + a button). No framework, no CDN —
  it has to work on a machine with nothing on it.
- `POST /enroll { code }` → call control's `/enroll` → write `/etc/sentinel-edge/edge.env`
  (`CONTROL_URL`, `BOX_TOKEN`, `TS_AUTHKEY`, `TS_HOSTNAME`) → reply OK → `exit(0)`. The supervisor
  (launchd `KeepAlive` / systemd `Restart=always`) brings it back enrolled — the same trick the
  Restart button already relies on.
- `CONTROL_URL` defaults to production instead of `127.0.0.1:8790` when a `SENTINEL_CONTROL_URL`
  build/env default is set by the installer.

## Phase 3 — web

- Boxes page: after **Add box**, lead with the **pairing code**, large, plus "run the installer on
  the appliance and enter this code". Poll the box's `last_seen` until it checks in, then flip to a
  success state — the user watches the box come alive instead of wondering.
- Demote the `curl … | sudo sh` command to an **Advanced / headless** disclosure. It still works;
  it's just no longer what a customer sees first.

## Verification

- Create a box → get a code → run a **fresh** edge with no `BOX_TOKEN` → open `localhost:8788` →
  type the code → the box appears in the Boxes page, private link `serving`, cameras pulled.
- Redeem the same code twice → the second is refused.
- Redeem an expired code → refused, with the same message as a wrong one.
- Wrong code, 20 times → rate-limited.
- The old `curl | sudo sh` one-liner still enrolls a box (nothing regressed).

## Open decisions (grill before building)

1. **Does the code create the box, or does the box already exist?** Above, the box row is created
   first (as today) and the code binds to it. The alternative — a code that creates the box on
   redeem — means an unenrolled box has no row to show in the UI, so you can't watch it arrive.
   Leaning: box first.
2. **Should redeem rotate the token even when `POST /boxes` already issued one?** Yes as planned —
   it keeps *nothing* secret at rest. The cost: a box enrolled by the one-liner and then paired by
   code would have its first token invalidated. Acceptable (they're alternative paths), but say so
   in the UI.
3. **Rate limiting: in-memory or Postgres?** In-memory per-IP is enough for one control pod and has
   no moving parts; it resets on deploy. Postgres survives restarts but puts a write on an
   unauthenticated path — a DoS lever. Leaning in-memory.
4. **Bind the enrollment page to localhost only?** Yes by default (the installer opens it locally).
   A headless appliance with no keyboard needs LAN access instead — that's a real case for a mini
   PC in a closet, and it argues for an `--enroll-bind 0.0.0.0` escape hatch. Decide before #82.

## Out of scope (v1)

- Re-pairing an already-enrolled box.
- QR codes / phone-based pairing.
- Any change to what the box does *after* it's enrolled.
