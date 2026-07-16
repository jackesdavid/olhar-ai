# Edge tailnet self-heal — re-run `up`, not just `serve`

**Board card:** #92 Edge tailnet self-heal — a stopped Tailscale strands the box (In Progress)
**Goal:** A box whose Tailscale stops for any reason should rejoin the tailnet **by itself**, within
one discovery loop. Today it strands permanently and the only way back is re-enrolling — a support
call for something the box can fix on its own.

Found while verifying [[camera-credential-wizard]] (#88) on the Mac test box, 2026-07-16.

---

## Why (the gap)

`reassert()` (`sentinel-edge/src/tailscale.rs`) runs every discovery loop and its doc comment claims
the box "heals ITSELF within minutes with no restart and no terminal… what makes the live path
self-recover for a real customer."

It doesn't. It re-runs `serve_go2rtc()` + `serve_edge_api()` and re-reports the MagicDNS host — but
it **never re-runs `tailscale up`**. `up --reset --authkey=…` happens *only* in `ensure_up()`, called
once at boot/enrollment. And `serve` on a node that isn't `Running` just fails.

So the failure mode is total and permanent:

- Tailscale stops (user quits the app, reboot, OS update, daemon crash) →
- `serve` fails every loop, forever → `tailscale_state()` reports `offline` →
- control's `box_host_for_tenant` still resolves a host, but nothing answers →
- **Live, snapshots, and the #88 credential probe are all dead**, with no journey back short of
  re-enrolling the box.

Observed exactly this: `tenant_boxes` = `MacInstalledBox | 0.5.40 | offline` while `last_seen` was
*current* — the box was happily polling control over HTTPS the whole time. That's the giveaway: the
config poll doesn't need the tailnet, so the box looks alive and healthy while every tailnet-side
feature is broken.

## The fix

In `reassert()`, check `node_running()` (extracted from `tailscale_state()`, which already computed
it) before doing anything else. If the node isn't `Running`, call `ensure_up()` — which re-runs
`up --reset --authkey`, waits for the hostname to settle, re-serves both ports, and re-reports the
host — then return. Only when the node *is* Running do we take the cheap `serve`-only path.

`tailscale_state()` now reuses `node_running()` rather than duplicating the `BackendState` parse.

Anything unreadable (CLI missing, daemon stopped, malformed JSON) counts as **not running** — the
safe reading, since that's exactly when re-asserting `up` is wanted.

## Verification

- Stop Tailscale on a box with `TS_AUTHKEY` set → within one discovery loop the log shows
  `tailscale node is not Running — re-running up to rejoin the tailnet`, the node rejoins, and
  `tenant_boxes.tailscale_state` returns to `serving` with no human action.
- A healthy box takes the `serve`-only path (one extra `status --json` per loop — cheap).
- A box with no `TS_AUTHKEY` (LAN/dev) still returns early: unchanged, no Tailscale calls.

## Known limits

- **macOS GUI-app installs**: the daemon is owned by the Tailscale app, so if the user *quits the
  app*, CLI `up` may not be able to restart the backend. `ensure_up` already tolerates a non-zero
  `up` and proceeds to `serve`, so this is no worse than today — but the appliance targets (Linux,
  the Windows service) are where this fix bites, and there it should fully self-heal.
- A **single-use `TS_AUTHKEY` that's already been consumed** can make `up` fail if the node was fully
  logged out (vs merely stopped). A stopped-but-still-authenticated node — the common case — comes
  back fine. Re-key/rotate is [[box-rekey-enrollment]] (#4) territory.

## Out of scope

- Surfacing "your box's tailnet is down" in the UI. `tailscale_state` is already reported and the
  Boxes page can render it; the honest-error copy for the live path is #89's Phase 3.
- Reworking enrollment or key rotation (#4).
