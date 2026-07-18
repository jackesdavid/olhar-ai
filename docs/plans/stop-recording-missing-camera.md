# Bug — Stop Recording: "missing required input 'camera'" on the DelayTrigger trailing edge

**Board card:** #114 Stop Recording missing camera on DelayTrigger stop (Bugs)
**Related:** [[delay-trigger-recording-signal-flow]] (#86), [[workflow-graph-instances]] (#95),
[[feedback_preroll_race]], migration 0058 (`values_json`)

## Symptom (reported 2026-07-17, "Recording By Movement" graph)

On the trailing edge the run logs:
```
Delay Trigger: window closed → stop
Get Variable  get $current_camera      (0 ms)
Get Variable  get $current_clip_id
Get Variable  get $pre_roll_seconds
Get Variable  get $current_camera
Stop Recording: missing required input 'camera'
```
The graph **is** wired: `Get current_camera.value → Stop Recording.camera`. Yet Stop Recording sees
no camera. The leading/extend runs (motion events) work fine — only the timer-fired **stop** run fails.

## Root cause (confirmed in code)

`DelayTrigger`'s `stop` fires on the **trailing edge in a SEPARATE run** — a timer-scheduled run, not
the motion run that armed/extended the window. Workflow variables live on `ToolContext::variables`,
which is a **"fresh empty map per run"** (`agent/tools.rs:103`, `:142`). `Set current_camera` ran in
the *motion* run; the *stop* run starts with an empty variables map. So `Get current_camera` in the
stop run returns its **type-zero (empty string)** → `Stop Recording.camera` (required) is missing.

The Get nodes DO execute in the stop run (they appear in the log) — they just read an unset variable.
Migration 0058 (`values_json`) persisted node **output values** across the delay window, but the
**variables map** is still per-run and is not restored into the trailing-edge stop run. That's the gap.

## Fix directions (decide at build)

1. **Persist + restore the variables map across the DelayTrigger window.** When the window is armed,
   snapshot `sys.variables` (or at least the vars a Set wrote) alongside the node `values_json`
   (same `workflow_node_state` row, keyed by tenant/graph/node/instance_key). On the trailing-edge
   stop run, seed `sys.variables` from that snapshot before resolving the stop subtree. Mirrors how
   0058 restored node outputs. **Leading candidate** — smallest, matches the existing mechanism.
2. **Carry the needed values on the DelayTrigger's own ports** (start/stop payload) instead of via
   variables — pass camera/clip_id through the node so the stop edge has them without a variable read.
   Bigger graph-shape change; conflicts with the "variables decouple wires" ergonomics the user wants.
3. **Instance-scoped variables (#95)** — if variables become per-instance persisted (not per-run),
   the stop run in the same instance reads what the start run wrote. The most general fix; largest.

## Verify

- Re-run "Recording By Movement": motion → start, quiet → **stop succeeds** with the right camera +
  clip_id; the clip closes. Check the stop run's `Get current_camera` resolves the armed camera, not "".
- A second concurrent camera's window doesn't cross wires (instance_key isolation holds).
