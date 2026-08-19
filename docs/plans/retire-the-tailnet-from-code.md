# Retire the tailnet from the code

**Board card:** #199 Tailnet/Tailscale removed from the code, not just the architecture
**Goal:** The tailnet was retired as an architecture in #144, but the code still carried it. Remove
the remnants so what the repo says matches what the system does.

## What was still there

`grep -i tailnet|tailscale` found **119 references across 36 files** — not all comments:

| Live machinery | Where |
|---|---|
| `tailscaled` started inside the edge container | `deploy/edge-linux/entrypoint.sh` |
| Tailscale package installed into the image | `deploy/edge-linux/Dockerfile` |
| `TS_AUTHKEY` / `TS_HOSTNAME` written at enrollment | `sentinel-edge/src/enroll.rs` |
| `LiveStart::Tailnet` variant + fallback dial path | `sentinel-gateway/src/{state,proxy}.rs` |
| `tailscale_state` column read/written on every heartbeat | `sentinel-control/src/store.rs` |
| `"tailnet"` live-start mode string | `sentinel-control/src/domain/boxes.rs` |
| User-facing copy naming the tailnet | `sentinel-app/.../i18n/messages/devices.ts` |

## The bug this uncovered

`tailscale_state` was bound as `$4`, and `$4` did double duty:

```sql
tailscale_state = COALESCE($4, tailscale_state),
health_at       = CASE WHEN $4 IS NOT NULL THEN now() ELSE health_at END
```

The edge stopped sending `X-Edge-Tailscale` when the tailnet was retired — `with_health` now sets
only Gstreamer, Platform and Metrics. So `$4` is always NULL, the CASE never fires, and
**`health_at` has not been stamped since**. It is written in exactly two places, both gated on that
same dead parameter, so nothing else was keeping it fresh. Box health timestamps on the Boxes page
have been frozen, silently.

Fixed by gating `health_at` on the GStreamer flag, which rides the same ~25s config poll and is
absent on the high-rate pushes — the identical semantics the tailscale param used to have.

## What replaced the naming

`Tailnet` → `Direct` for the gateway's non-SSE path, and the live-start mode string `"tailnet"` →
`"direct"`. That path is not dead: it still reaches a LAN box in dev and the static upstream. What
is dead is reaching a *deployed* edge that way, and the comments now say so instead of implying a
VPN is carrying it.

## Deliberately left alone

- **The `tailscale_state` column itself.** Migrations are immutable history; dropping a column needs
  a new migration, and a bad migration is a boot-retry loop. Nothing reads or writes it now — it is
  inert. Drop it in its own change, validated against a live database first.
- **`docs/`** — the reference notes still describe the tailnet era. Worth a pass, but prose that is
  merely outdated is not the same as code that still ships a VPN daemon.
- **`legacy/`** — dead Tauri-era code, ignored by convention.

## Verification criteria

1. `grep -riE "tailnet|tailscale"` over code (excluding `docs/`, `legacy/`) returns nothing.
2. `cargo check` clean for the workspace AND the out-of-workspace edge crate.
3. Gateway + app-core unit tests pass.
4. `health_at` advances on a box heartbeat — **needs checking against the live DB after deploy**,
   since the freeze was invisible precisely because nothing tested it.
