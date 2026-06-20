-- Floor-plan geometry beyond rooms: free-form WALLS (a segment A→B with a
-- thickness; thin = a drawn line, thick = a solid wall) and MARKERS (furniture /
-- landmark SVG glyphs). Rooms stay labeled zones (map_rooms) — walls are NOT
-- implied by a room, so open spaces (backyard, open garage) simply have none.
-- Mirrors map_rooms: one jsonb row per item, tenant-scoped, replace-all on save.
CREATE TABLE IF NOT EXISTS map_walls (
    tenant_id  uuid   NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    id         bigint NOT NULL,   -- MapWall.id (per-tenant)
    data       jsonb  NOT NULL,   -- full serialized MapWall (x1,y1,x2,y2,thickness)
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE IF NOT EXISTS map_markers (
    tenant_id  uuid   NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    id         bigint NOT NULL,   -- MapMarker.id (per-tenant)
    data       jsonb  NOT NULL,   -- full serialized MapMarker (icon,x,y,rot,scale,label)
    PRIMARY KEY (tenant_id, id)
);
