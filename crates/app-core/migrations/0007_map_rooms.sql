-- Floor-plan rooms → Postgres (Phase 0). One row per MapRoom, stored as its
-- serialized JSON. Replace-all on save mirrors the devices table.
CREATE TABLE IF NOT EXISTS map_rooms (
    id    bigint PRIMARY KEY,   -- MapRoom.id
    data  jsonb   NOT NULL      -- the full serialized MapRoom (name, x/y/w/h, cameras)
);
