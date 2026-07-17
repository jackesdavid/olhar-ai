-- An instance's value context: what each node last produced (cards #95 phase 3, #94).
--
-- Same row as the node's latent state, deliberately — "everything in the same place". But a
-- SEPARATE column, because state_json is the window and its writers rewrite the whole document:
-- try_arm_node_state does `SET state_json = EXCLUDED.state_json` and the disarm does
-- `SET state_json = '{"state":"idle"}'`. Values living in there would be wiped by the very
-- transitions they have to outlive — the window closing is precisely when the stop branch needs
-- them.
--
-- Why this exists at all: a DelayTrigger's `stop` is delivered by a timer, in a DIFFERENT run from
-- the `start`. StopRecording.camera comes from OnEvent and its clip_id from StartRecording, and
-- neither node runs on the trailing edge, so both inputs resolved to nothing and the engine skipped
-- the branch as dead — silently, with the run reporting ok. The graph was right all along; the
-- engine had thrown the values away.
--
-- Keyed by the same (tenant, graph, node, instance_key) as the state, so camera A's clip_id can't
-- be read by camera B's stop.
ALTER TABLE workflow_node_state
    ADD COLUMN IF NOT EXISTS values_json TEXT NOT NULL DEFAULT '{}';
