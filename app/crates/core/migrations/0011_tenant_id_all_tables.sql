-- Phase 2: make all domain data multi-tenant. Add tenant_id to every table, rework
-- primary keys to be per-tenant (composite), and backfill existing rows to a Default
-- tenant so single-tenant callers (the desktop app, sentinel-flow) keep working while
-- the cloud APIs (Phase 3) start passing the real tenant from the JWT.
--
-- The Default tenant id is the well-known UUID exposed as app_core::tenancy::DEFAULT_TENANT_ID.

-- The backfill target. (tenants table created in 0010.)
INSERT INTO tenants (id, name, slug, status)
VALUES ('11111111-1111-1111-1111-111111111111', 'Default', 'default', 'active')
ON CONFLICT (id) DO NOTHING;

-- ---- devices: PK ip → (tenant_id, ip) --------------------------------------
ALTER TABLE devices ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE devices SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE devices ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE devices ADD CONSTRAINT devices_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE devices DROP CONSTRAINT devices_pkey;
ALTER TABLE devices ADD PRIMARY KEY (tenant_id, ip);

-- ---- persons + person_embeddings (rework the self-FK + embeddings FK) -------
ALTER TABLE person_embeddings DROP CONSTRAINT person_embeddings_person_id_fkey;
ALTER TABLE persons DROP CONSTRAINT persons_parent_id_fkey;

ALTER TABLE persons ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE persons SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE persons ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE persons ADD CONSTRAINT persons_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE persons DROP CONSTRAINT persons_pkey;
ALTER TABLE persons ADD PRIMARY KEY (tenant_id, id);
-- self-hierarchy within a tenant (parent_id nullable → MATCH SIMPLE skips null rows)
ALTER TABLE persons ADD CONSTRAINT persons_parent_fkey
    FOREIGN KEY (tenant_id, parent_id) REFERENCES persons(tenant_id, id) ON DELETE SET NULL;

ALTER TABLE person_embeddings ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE person_embeddings SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE person_embeddings ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE person_embeddings ADD CONSTRAINT person_embeddings_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE person_embeddings ADD CONSTRAINT person_embeddings_person_fkey
    FOREIGN KEY (tenant_id, person_id) REFERENCES persons(tenant_id, id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS person_embeddings_tenant_idx ON person_embeddings(tenant_id);

-- ---- alerts: PK id → (tenant_id, id) (per-tenant monotonic) -----------------
ALTER TABLE alerts ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE alerts SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE alerts ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE alerts ADD CONSTRAINT alerts_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE alerts DROP CONSTRAINT alerts_pkey;
ALTER TABLE alerts ADD PRIMARY KEY (tenant_id, id);

-- ---- notification_targets: PK chat_id → (tenant_id, chat_id) ----------------
ALTER TABLE notification_targets ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE notification_targets SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE notification_targets ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE notification_targets ADD CONSTRAINT notification_targets_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE notification_targets DROP CONSTRAINT notification_targets_pkey;
ALTER TABLE notification_targets ADD PRIMARY KEY (tenant_id, chat_id);

-- ---- workflows: PK id → (tenant_id, id) ------------------------------------
ALTER TABLE workflows ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE workflows SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE workflows ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE workflows ADD CONSTRAINT workflows_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE workflows DROP CONSTRAINT workflows_pkey;
ALTER TABLE workflows ADD PRIMARY KEY (tenant_id, id);

-- ---- map_rooms: PK id → (tenant_id, id) ------------------------------------
ALTER TABLE map_rooms ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE map_rooms SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE map_rooms ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE map_rooms ADD CONSTRAINT map_rooms_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE map_rooms DROP CONSTRAINT map_rooms_pkey;
ALTER TABLE map_rooms ADD PRIMARY KEY (tenant_id, id);

-- ---- telegram_config: per-tenant (was singleton id=1) ----------------------
ALTER TABLE telegram_config ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE telegram_config SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE telegram_config ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE telegram_config ADD CONSTRAINT telegram_config_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE telegram_config DROP CONSTRAINT telegram_config_pkey;
ALTER TABLE telegram_config ADD PRIMARY KEY (tenant_id);

-- ---- workflow_engine_state: per-tenant (was singleton id=true) --------------
ALTER TABLE workflow_engine_state ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE workflow_engine_state SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE workflow_engine_state ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE workflow_engine_state ADD CONSTRAINT workflow_engine_state_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE workflow_engine_state DROP CONSTRAINT workflow_engine_state_pkey;
ALTER TABLE workflow_engine_state ADD PRIMARY KEY (tenant_id);

-- ---- workflow_cron_state: PK graph_id → (tenant_id, graph_id) ---------------
ALTER TABLE workflow_cron_state ADD COLUMN IF NOT EXISTS tenant_id uuid;
UPDATE workflow_cron_state SET tenant_id = '11111111-1111-1111-1111-111111111111' WHERE tenant_id IS NULL;
ALTER TABLE workflow_cron_state ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE workflow_cron_state ADD CONSTRAINT workflow_cron_state_tenant_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE workflow_cron_state DROP CONSTRAINT workflow_cron_state_pkey;
ALTER TABLE workflow_cron_state ADD PRIMARY KEY (tenant_id, graph_id);
