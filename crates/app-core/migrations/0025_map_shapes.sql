CREATE TABLE IF NOT EXISTS map_shapes (
    tenant_id UUID NOT NULL,
    id        BIGINT NOT NULL,
    data      JSONB NOT NULL,
    PRIMARY KEY (tenant_id, id)
);
