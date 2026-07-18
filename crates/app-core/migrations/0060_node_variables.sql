-- A window's VARIABLES snapshot (card #114).
--
-- Migration 0058 (`values_json`) closed the node-OUTPUT half of the trailing-edge bug:
-- StopRecording.camera (from OnEvent) and .clip_id (from StartRecording) survive into the timer-
-- scheduled stop run. But workflow VARIABLES (`Get`/`Set Variable`, sys.variables) are a fresh,
-- empty map per run — seeded only with the graph's declared defaults. So on the trailing edge a
-- `Get current_camera` read a type-zero (empty string) for whatever a `Set current_camera` wrote on
-- the arming run, and StopRecording's required `camera` input resolved to nothing → the branch was
-- skipped as dead, silently, with the run reporting ok. The graph was right; the engine had thrown
-- the variables away.
--
-- Same row as the node's state + values, a SEPARATE column for the same reason values_json is
-- separate: the window's writers rewrite state_json whole. Snapshotted on the DelayTrigger's own row
-- when it arms/extends, restored onto sys.variables at the start of the trailing-edge stop run.
--
-- Keyed by the same (tenant, graph, node, instance_key) as the state and values, so camera A's
-- variables can't be read by camera B's stop.
ALTER TABLE workflow_node_state
    ADD COLUMN IF NOT EXISTS variables_json TEXT NOT NULL DEFAULT '{}';
