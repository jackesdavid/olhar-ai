-- #174 step 1: per-tenant settings, starting with the face capture stream.
--
-- Until now every knob the edge reads lived in `tenant_boxes.recording_config` / `tracking_config`
-- — JSONB **per box**. That is right for things a box genuinely owns (its disk quota, its cameras),
-- and wrong for a product choice the customer makes once: "capture faces from the HD stream".
-- Expressed per box, the same decision has to be repeated on every appliance a tenant installs, and
-- silently diverges the moment one is added.
--
-- One row per tenant, settings as JSONB rather than columns. #128 (per-tenant/camera similarity
-- thresholds) is the next thing that needs this shape, and adding a column per knob would mean a
-- migration for each. JSONB with a documented key set is the cheaper contract here — these are
-- preferences read at request time, never joined or aggregated.
--
-- Resolution is server-side: `recording_config_by_token` overlays the tenant value onto the box's
-- recording_config, so the EDGE IS UNCHANGED. It keeps reading `face_capture_stream` from the same
-- place; only who decides the value moves. That is deliberate — a change requiring every appliance
-- in the field to update would gate a UI preference on an edge release.
--
-- Known keys:
--   face_capture_stream : 'preview' | 'hd'  — which stream faces are captured from (#120/#174)
CREATE TABLE IF NOT EXISTS tenant_settings (
    tenant_id  uuid PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    settings   jsonb NOT NULL DEFAULT '{}'::jsonb,
    updated_at text NOT NULL
);
