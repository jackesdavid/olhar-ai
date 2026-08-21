# Retire the private link from the code

**Board card:** #199 Retire the private-link remnants from the code, not just the architecture
**Goal:** The private mesh was retired as an architecture in #144, but the code still carried it. Remove
the remnants so what the repo says matches what the system does.

## What was still there

`grep -i` for the old names found **119 references across 36 files** — not all comments:

| Live machinery | Where |
|---|---|
| the VPN daemon started inside the edge container | `deploy/edge-linux/entrypoint.sh` |
| the VPN package installed into the image | `deploy/edge-linux/Dockerfile` |
| `TS_AUTHKEY` / `TS_HOSTNAME` written at enrollment | `sentinel-edge/src/enroll.rs` |
| `LiveStart::Private link` variant + fallback dial path | `sentinel-gateway/src/{state,proxy}.rs` |
| the state column read/written on every heartbeat | `sentinel-control/src/store.rs` |
| `"private link"` live-start mode string | `sentinel-control/src/domain/boxes.rs` |
| user-facing copy naming the plumbing | `sentinel-app/.../i18n/messages/devices.ts` |

## The bug this uncovered

The state column was bound as `$4`, and `$4` did double duty:

```sql
<state_col> = COALESCE($4, <state_col>),
health_at   = CASE WHEN $4 IS NOT NULL THEN now() ELSE health_at END
```

The edge stopped sending that header when the private mesh was retired — `with_health` now sets
only Gstreamer, Platform and Metrics. So `$4` is always NULL, the CASE never fires, and
**`health_at` has not been stamped since**. It is written in exactly two places, both gated on that
same dead parameter, so nothing else was keeping it fresh. Box health timestamps on the Boxes page
have been frozen, silently.

Fixed by gating `health_at` on the GStreamer flag, which rides the same ~25s config poll and is
absent on the high-rate pushes — the identical semantics the old parameter used to have.

## What replaced the naming

The gateway's non-SSE path was renamed to `Direct`, and the live-start mode string →
`"direct"`. That path is not dead: it still reaches a LAN box in dev and the static upstream. What
is dead is reaching a *deployed* edge that way, and the comments now say so instead of implying a
VPN is carrying it.

## Deliberately left alone

- **The inert state column itself.** Migrations are immutable history; dropping a column needs
  a new migration, and a bad migration is a boot-retry loop. Nothing reads or writes it now — it is
  inert. Drop it in its own change, validated against a live database first.
- **Migrations** — sqlx checksums every migration file, so editing even a comment in one breaks boot. Worth a pass, but prose that is
  They are immutable history and stay exactly as written.
- **`legacy/`** — dead Tauri-era code, ignored by convention.

## Verification criteria

1. A case-insensitive grep for the old names returns nothing outside migrations and `legacy/`.
2. `cargo check` clean for the workspace AND the out-of-workspace edge crate.
3. Gateway + app-core unit tests pass.
4. `health_at` advances on a box heartbeat — **VALIDATED 2026-08-20 against the live DB.**
   `Jack Mac` (edge 0.5.73, which does not send the old header, so the old code could never have
   stamped it) reported `health_at` 9 seconds old. `Silicom Appliance` carries the bug's
   fingerprint: `health_at` frozen at 2026-07-29, **two days older than its own `last_seen`** — it
   went on heartbeating while health quietly stopped advancing.
