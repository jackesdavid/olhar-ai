# Box Re-key / Rotate Enrollment Action — Implementation Plan

**Board card:** Box re-key / rotate enrollment action (Backlog)
**Ref:** docs/ref/edge.md (Install / env → enrollment key)
**Goal:** Re-mint a reusable Tailscale auth key for an existing box **without recreating it** — keep the box id / token / cameras, push the new key into `edge.env`, and restart the appliance.

## Context

Enrollment keys now mint **reusable + non-ephemeral + 90d** (`sentinel-control/src/tailscale.rs`, fixed in `20814c1`), so a reboot recovers the tailnet membership. But after the ~90-day key lifetime a box still needs re-enrollment, and today the **only** recovery path is delete + recreate + reinstall — which loses the box id/token/camera bindings.

## Phases

### Phase 1 — Re-key action (backend)
- Add a control-plane action that, for an existing box, mints a fresh reusable/non-ephemeral Tailscale auth key (reuse the minting path in `sentinel-control/src/tailscale.rs`) **without** touching the box registry row (id, token, cameras stay).
- Return the new key so it can be pushed into the appliance's `edge.env`.

### Phase 2 — Push + restart (edge)
- Deliver the new key into `edge.env` and restart the edge service (reuse the existing restart command path).
- Optionally disable node-key expiry so appliances survive past the ~90-day key lifetime.

### Phase 3 — Boxes page UI
- Surface a "Re-key / rotate enrollment" action on the Boxes page for an existing box (web-only shadcn dialog, no `window.confirm`).

## Open decisions
1. Disable node-key expiry by default, or keep the 90d lifetime and just make rotation one click?
2. Does the new key reach the appliance via the existing pull/health channel, or a dedicated push?

## Out of scope (v1)
- Automatic pre-expiry rotation (proactively re-keying before 90d lapses).
