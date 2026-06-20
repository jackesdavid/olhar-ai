-- Per-node persistent state (used by stateful flow-control nodes like Flip Flop).
CREATE TABLE IF NOT EXISTS workflow_node_state (
    tenant_id  UUID    NOT NULL,
    graph_id   INTEGER NOT NULL,
    node_id    INTEGER NOT NULL,
    state_json TEXT    NOT NULL DEFAULT '{}',
    PRIMARY KEY (tenant_id, graph_id, node_id)
);
