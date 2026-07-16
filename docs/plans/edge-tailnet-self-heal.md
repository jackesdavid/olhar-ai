# Edge tailnet self-heal — re-run `up`, not just `serve`

**Board card:** #92 Edge tailnet self-heal — a stopped Tailscale strands the box (In Progress)
**Goal:** A box whose Tailscale stops for any reason should rejoin the tailnet **by itself**, within
one discovery loop — and a Tailscale that is stopped or wedged must never take the *appliance* down
with it. A dead tailnet should cost us the tailnet, never the box.

Found while verifying [[camera-credential-wizard]] (#88) on the Mac test box, 2026-07-16.

> **Scope grew mid-card.** It began as "the box won't rejoin" (`reassert` never re-ran `up`). Then
> the same box, told to Update from the UI, went completely silent — and the cause was worse than
> not-rejoining: **a hung `tailscale` CLI call was hanging the edge's whole boot.** Both fixes are
> here; the hang is the more serious of the two.

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

## Fault 2 (worse): a hung `tailscale` call bricks the box

`ts()` builds a `Command` and every caller ran it with `.output()` / `.status()` — **which wait
forever**. And the CLI *does* hang: with the daemon stopped, `tailscale serve --bg 1984` never
returns. Caught in the act on the test box:

```
64972  20:54  Ss  /opt/sentinel-edge/sentinel-edge
 └─ 65223  20:32  S   tailscale serve --bg 1984      <-- hung 20 minutes
```

`ensure_up()` is called from `main.rs:146` — **before** `AppState::boot()` and before the HTTP
server. So one hung `serve` doesn't just cost the tailnet, it stops the edge booting at all:

- port 8788 never opens (no edge API),
- the config poll never runs → `last_seen` freezes, control still shows the box as merely `offline`,
- **the box can no longer be told to restart, and can never self-update** — the two journeys that
  would deliver this very fix.

Bricked, on a box the user was actively trying to update. The Update button appeared to do nothing
because the edge exited (as designed), relaunched, and re-hung ~22s into boot on the same call.

**Fix:** a `run(cmd, what)` helper that spawns, polls `try_wait`, and **kills the child** at
`TS_TIMEOUT` (15s), returning `None`. Every tailscale call now goes through it — `up`, both `serve`s,
`serve status`, `status --json` (via `node_running` and `detect_dns_name`, which `wait_for_hostname`
polls). None left with an unbounded wait.

## The fix (fault 1: never rejoins)

In `reassert()`, check `node_running()` (extracted from `tailscale_state()`, which already computed
it) before doing anything else. If the node isn't `Running`, call `ensure_up()` — which re-runs
`up --reset --authkey`, waits for the hostname to settle, re-serves both ports, and re-reports the
host — then return. Only when the node *is* Running do we take the cheap `serve`-only path.

`tailscale_state()` now reuses `node_running()` rather than duplicating the `BackendState` parse.

Anything unreadable (CLI missing, daemon stopped, malformed JSON) counts as **not running** — the
safe reading, since that's exactly when re-asserting `up` is wanted.

## Verification

- **The hang**: stop the Tailscale daemon, restart the edge → boot must complete: the log shows
  `tailscale serve --bg 1984 hung for 15s — killing it and carrying on`, port 8788 opens, and the
  config poll resumes. The box stays reachable and updatable with the tailnet dead — the whole point.
- Stop Tailscale on a box with `TS_AUTHKEY` set → within one discovery loop the log shows
  `tailscale node is not Running — re-running up to rejoin the tailnet`, the node rejoins, and
  `tenant_boxes.tailscale_state` returns to `serving` with no human action.
- A healthy box takes the `serve`-only path (one extra `status --json` per loop — cheap).
- A box with no `TS_AUTHKEY` (LAN/dev) still returns early: unchanged, no Tailscale calls.

## The recovery trap (why this had to ship as its own release)

A box already hung *cannot* pick this up: it can't poll, so it can't self-update to the version that
fixes the hang. The first box needs Tailscale started by hand once; after that it's immune. Any fix
to a boot-path hang has this property — worth remembering before putting anything else that can
block ahead of the config poll.

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
