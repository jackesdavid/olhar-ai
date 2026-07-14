-- Box pairing codes (card #81) — how a box learns who it belongs to without four secrets on a
-- command line.
--
-- Installing a box used to mean pasting `--control … --box-token … --authkey … --hostname …` onto
-- the appliance. A double-clicked .pkg/.exe has no way to receive that, and baking the token into a
-- per-box installer is a dead end (a signed package can't be modified without breaking its
-- signature). So instead: the web app mints a short code, the person standing at the appliance
-- types it into the edge's local enrollment page, and the edge redeems it.
--
-- NOTHING SECRET LIVES HERE. We store only a hash of the code — never the code, never the box
-- token, never the Tailscale key. Redeeming *mints* a fresh box token (rotating
-- `tenant_boxes.token_hash`, which is already how tokens are stored) and a fresh Tailscale auth key
-- on the spot. A dump of this table yields nothing usable.

CREATE TABLE IF NOT EXISTS box_pairings (
    -- sha256 of the pairing code. The code itself is shown once, in the browser, and never stored.
    code_hash  text PRIMARY KEY,
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    box_id     uuid NOT NULL REFERENCES tenant_boxes(id) ON DELETE CASCADE,
    -- RFC3339 text, not timestamptz: sqlx is built without the chrono/time feature here and can't
    -- decode timestamptz (see CLAUDE.md). Generated with app_core::timeutil::now_rfc3339().
    expires_at text NOT NULL,
    -- Set exactly once, by the redeem that wins the race. A claimed code is spent.
    claimed_at text,
    created_at text NOT NULL
);

-- One live code per box: minting a new one supersedes the old (ON CONFLICT on box_id would need
-- this to be unique, but a box may legitimately have spent codes in history) — so index, not
-- constraint, and the mint path deletes this box's unclaimed codes first.
CREATE INDEX IF NOT EXISTS box_pairings_box_idx ON box_pairings(box_id);
-- The sweeper's access path: expired-and-unclaimed rows are garbage.
CREATE INDEX IF NOT EXISTS box_pairings_expires_idx ON box_pairings(expires_at) WHERE claimed_at IS NULL;
