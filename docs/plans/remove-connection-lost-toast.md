# Remove "Connection lost" startup toast — Implementation Plan

**Board card:** Remove "Connection lost" startup toast (ToDo)
**Goal:** Kill the persistent "Connection lost — reconnecting…" toast that wrongly pops right
after login while the SSE stream is still doing its first connect.

## Root cause
`sentinel-app/src/routes/+layout.svelte:186–203` — a Svelte 5 `$effect` that runs once
`auth.me` is set (right after login) and shows a **persistent** (`duration: Infinity`) toast on
a synthetic `stream-closed` event, dismissing it on `stream-open`.

Why it's wrong at startup: right after login the `EventSource` (`ensureCloudStream()` in
`sentinel-app/src/lib/data.ts:80–111`) is still opening. If that initial connection errors/closes
before the first `onopen` — common during boot, a cold relay, or a transient 502 — `stream-closed`
fires *before* `stream-open`, so the toast shows during normal startup. data.ts auto-retries after
3s (data.ts:107) and it dismisses itself, but the user has already seen a false "Connection lost".

This is the only place the string "Connection lost — reconnecting…" exists.

## The change
- Delete the entire `$effect` block at `+layout.svelte:186–203`, including the
  `// Stream reconnect…` comment on line 186. Nothing else references `disconnectToastId`.

## Do NOT touch
- `sentinel-app/src/lib/data.ts` — leave it fully intact. The `stream-open` / `stream-closed`
  events also drive state re-seeding on reconnect (presence, etc.) and the manual 3s retry loop
  (data.ts:107). Removing the layout effect stops only the *UI notification*; reconnect keeps
  working silently.
- `onEvent`, `toast`, and `<Toaster />` (`+layout.svelte:517`) stay — used by alert toasts and
  the update-available toast elsewhere in the layout.
- Unrelated: the read-only-mode banner (`+layout.svelte:246`) and per-device/edge "offline"
  health dots (`status-dot.svelte`, `api.ts`) are NOT this feature — leave them.

## Out of scope (v1)
- No replacement banner. If a genuine "you've been offline for a while" signal is wanted later,
  that's a separate card gated on a real sustained-disconnect condition, not first-connect noise.

## Verify
- Log in on a cold session → no "Connection lost" toast during startup.
- Kill the relay mid-session → reconnect still happens silently (state re-seeds) with no toast.
