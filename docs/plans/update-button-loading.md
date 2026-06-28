# Update Button: Stay Loading Until Complete — Implementation Plan

**Board card:** Update button stays loading until update completes (ToDo)
**Goal:** Keep the Update button in a loading/disabled state from click until the edge box reports the new version, with a timeout fallback.

## How the update works end-to-end

1. UI calls `POST /boxes/:id/update` → control sets `pending_update = TRUE`
2. Edge polls commands (~30s interval) → sees `update = true` → calls `exit(0)`
3. launchd restarts the edge → `maybe_self_update()` downloads new binary, re-execs via launchctl
4. Edge comes back up, next heartbeat sets `tenant_boxes.version` to the new version
5. `GET /boxes` returns the updated version → `b.version === latest` → done

Total elapsed: ~60–90 seconds.

## Current behaviour

`updatingId` is set to `b.id` for the duration of the API call only (~200ms). The button stops loading immediately after the POST returns, before the edge has done anything.

## Phase 1 — Poll until version matches

In `sentinel-app/src/routes/settings/boxes/+page.svelte`:

- After `boxApi.update(b.id)` succeeds, do **not** clear `updatingId` in `finally`.
- Start a polling loop: every 5 seconds call `load()` (already exists, refreshes `list` + `latest`).
- When the polled `list` shows `b.version === latest` for that box id → clear `updatingId`, show `toast.success("Updated to v{latest}")`, stop polling.
- Timeout: after 3 minutes (36 polls) give up, clear `updatingId`, show `toast.warning("Update is taking longer than expected — check the box logs")`.
- Clean up the interval on component destroy (`$effect` cleanup return).

```svelte
let updatePollStop: (() => void) | null = null;

async function updateBox(b: Box) {
  updatingId = b.id;
  try {
    await boxApi.update(b.id);
    toast.info(`Update started for "${b.name}" — waiting for v${latest}…`);
    startUpdatePoll(b.id);
  } catch (e) {
    updatingId = null;
    toast.error(…);
  }
  // no finally clear — poll clears it
}

function startUpdatePoll(boxId: string) {
  let attempts = 0;
  const id = setInterval(async () => {
    attempts++;
    await load();
    const box = list.find(b => b.id === boxId);
    if (box?.version === latest) {
      clearInterval(id);
      updatePollStop = null;
      updatingId = null;
      toast.success(`Edge updated to v${latest}`);
    } else if (attempts >= 36) {
      clearInterval(id);
      updatePollStop = null;
      updatingId = null;
      toast.warning("Update is taking longer than expected — check logs");
    }
  }, 5000);
  updatePollStop = () => clearInterval(id);
}
```

In the `$effect` cleanup:
```svelte
$effect(() => {
  if (allowed) load();
  else loading = false;
  return () => updatePollStop?.();
});
```

## Phase 2 — Button UX

- Button shows spinner + "Updating…" text while `updatingId === b.id`
- Button disabled (not just visually loading) so double-click is impossible
- The version badge next to the box name updates live as `list` refreshes

## Out of scope (v1)

- SSE push from control when heartbeat updates version (poll is sufficient and simpler)
- Progress bar / step indicators
- Per-box update history
