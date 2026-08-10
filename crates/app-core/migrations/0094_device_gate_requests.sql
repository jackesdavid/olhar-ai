-- Card #176 — Device Gate: a human approves, from their phone, before the graph continues.
--
-- One row per ENTRY into the node. The run that created it has already ended (see the map's
-- decision 03: a run cannot block on a person, and a `waiting` run would make a SkipIfRunning graph
-- refuse every trigger while a phone lies face-down). The answer, or the expiry, resumes at the
-- node via `spawn_node_subrun` — the same machinery DelayTrigger uses.
--
-- Timestamps are RFC3339 `text`, per the project rule: sqlx is built without the chrono/time
-- feature, so it cannot decode `timestamptz`.
CREATE TABLE IF NOT EXISTS device_gate_requests (
    -- The request id. Travels in the push payload and in the URL the phone answers on. Not secret
    -- on its own — `token_hash` is what makes an answer provable.
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,

    -- Where to resume. `node_id` is the per-graph integer the engine addresses.
    graph_id         uuid NOT NULL,
    node_id          integer NOT NULL,
    -- The run that ASKED. That run is over by the time anyone answers, so this is not a foreign key
    -- into live state — it is the audit join. Decision 03 accepted that run history shows the ask
    -- and the answer as two entries; this column is what lets an incident be reconstructed anyway.
    asked_by_run     uuid,

    -- Decision 01: ONLY the addressed device may answer. Not a user, not a permission. A device IS
    -- a Recipient (#139), so this is the Recipient the graph author picked, unchanged.
    -- CASCADE: if the phone is deleted, its pending gates are meaningless — nothing may answer them
    -- ever again, and leaving unanswerable rows around would only feed the expiry sweep forever.
    push_device_id   uuid NOT NULL REFERENCES push_devices(id) ON DELETE CASCADE,

    -- Decision 04: the SHA-256 hash, hex, never the token itself. A dump of this table must not be
    -- enough to open a gate. The raw token exists in the push payload and in memory during the
    -- request that mints it, nowhere else.
    token_hash       text NOT NULL,

    -- What the person reads, and which automation is asking. `flow_name` is copied from the graph
    -- at mint time rather than read back later: the graph can be renamed or deleted while a request
    -- is pending, and the phone must show what was true when the question was asked.
    prompt           text NOT NULL,
    flow_name        text NOT NULL,

    -- pending | approved | denied | expired | failed
    --
    -- Five states onto three exec arms (approved → Positive, denied → Negative, expired/failed →
    -- Failure), stored SEPARATELY rather than as the arm taken. The arm is what the graph branches
    -- on; the state is what the audit trail has to be able to say. "Nobody answered" and "the push
    -- was undeliverable" both take Failure and are not the same event.
    state            text NOT NULL DEFAULT 'pending',

    created_at       text NOT NULL,
    -- The TTL deadline. A sweep settles anything still pending past this and resumes down Failure.
    expires_at       text NOT NULL,

    answered_at      text,
    -- Who tapped. NULL-able so the row survives the user being deleted, and because expiry has no
    -- answerer at all.
    answered_by_user uuid REFERENCES users(id) ON DELETE SET NULL,
    -- Decision 02: `biometric` | `passcode`. A biometric is a property of the DEVICE, not a
    -- requirement of the product, so a phone with none falls back to the passcode — and recording
    -- WHICH is the only thing keeping that fallback from being a silent downgrade. An approval
    -- whose strength is unknowable is the outcome this column exists to prevent.
    auth_factor      text
);

-- The phone's own list: `GET /device-gate/pending`, scoped to the calling device (decision 01 —
-- not the user's other devices).
CREATE INDEX IF NOT EXISTS device_gate_requests_device_pending_idx
    ON device_gate_requests (push_device_id, state);

-- The expiry sweep, so finding what is due does not scan the table.
CREATE INDEX IF NOT EXISTS device_gate_requests_expiry_idx
    ON device_gate_requests (state, expires_at);

-- The canvas countdown reads the pending request for a node without scanning by tenant.
CREATE INDEX IF NOT EXISTS device_gate_requests_node_idx
    ON device_gate_requests (tenant_id, graph_id, node_id, state);
