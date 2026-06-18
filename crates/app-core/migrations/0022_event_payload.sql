-- Typed payload schema for tenant events: a list of {name, type} fields an event
-- carries. EmitEvent gains an input port per field; OnEvent an output port per field.
-- Existing events default to '[]' (payload-less) — no behaviour change for them.
ALTER TABLE tenant_events
    ADD COLUMN IF NOT EXISTS payload jsonb NOT NULL DEFAULT '[]'::jsonb;
