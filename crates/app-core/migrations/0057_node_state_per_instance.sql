-- Scope a node's latent state to an INSTANCE, not just a node (card #95).
--
-- The old key was (tenant_id, graph_id, node_id), so a DelayTrigger had exactly one window ever,
-- no matter how many contexts were live. Two cameras through one recording graph: camera A's
-- motion armed the window, camera B's `try_arm` then failed ("already armed") and B silently
-- EXTENDED A's window instead of opening its own. One stop fired, in one context — B's clip was
-- never stopped. Single-camera graphs worked by accident: with one context, "the node's window"
-- and "this context's window" are the same row.
--
-- instance_key is the graph's InstanceScope applied to the firing signal:
--   Single    (default) -> ''            one instance per graph = exactly the old behaviour
--   PerCamera           -> the camera id  one window per camera, so A and B stop their own clips
--
-- DEFAULT '' + backfilling existing rows to '' means every graph alive today keeps its exact
-- semantics across this migration: their scope is Single, their key is '', and they address the
-- same row they always did.
ALTER TABLE workflow_node_state
    ADD COLUMN IF NOT EXISTS instance_key TEXT NOT NULL DEFAULT '';

ALTER TABLE workflow_node_state DROP CONSTRAINT IF EXISTS workflow_node_state_pkey;
ALTER TABLE workflow_node_state
    ADD CONSTRAINT workflow_node_state_pkey
    PRIMARY KEY (tenant_id, graph_id, node_id, instance_key);
