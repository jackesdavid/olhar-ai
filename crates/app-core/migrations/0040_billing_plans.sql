-- Subscription plans + per-tenant subscriptions. A plan is a fixed BRL base fee + a
-- camera cap + optional trial window + included quotas / overage rates. v1 tiers are
-- flat and all-inclusive (included_* = overage_* = 0 → invoice = base fee); the quota/
-- overage columns exist so a future usage-based tier can bill excess without a migration.
-- Money is double precision (sqlx has no numeric decoder); timestamps are RFC3339 text.

CREATE TABLE IF NOT EXISTS billing_plans (
    id                        text PRIMARY KEY,            -- slug: 'beta' | 'home' | 'business'
    name                      text NOT NULL,
    base_fee_brl              double precision NOT NULL DEFAULT 0,
    camera_cap                integer NOT NULL,
    trial_days                integer,                     -- NULL = not a trial plan
    included_gpu_sec          double precision NOT NULL DEFAULT 0,
    included_storage_gb       double precision NOT NULL DEFAULT 0,
    included_notifications    integer NOT NULL DEFAULT 0,
    overage_gpu_per_sec       double precision NOT NULL DEFAULT 0,
    overage_storage_per_gb    double precision NOT NULL DEFAULT 0,
    overage_notification_each double precision NOT NULL DEFAULT 0,
    active                    boolean NOT NULL DEFAULT true
);

-- One row per (tenant, plan) span. Current plan = the tenant's newest row. A trial has
-- status 'trialing' and ended_at = started_at + trial_days; paid plans are open-ended
-- (ended_at NULL) until changed/canceled.
CREATE TABLE IF NOT EXISTS tenant_subscriptions (
    id         uuid PRIMARY KEY,
    tenant_id  uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    plan_id    text NOT NULL REFERENCES billing_plans(id),
    status     text NOT NULL,
    started_at text NOT NULL,
    ended_at   text,
    CONSTRAINT tenant_subscriptions_status_chk
        CHECK (status IN ('trialing', 'active', 'past_due', 'canceled', 'expired'))
);

CREATE INDEX IF NOT EXISTS tenant_subscriptions_tenant_idx
    ON tenant_subscriptions (tenant_id, started_at DESC);

-- Seed the launch tiers. Flat + all-inclusive (no overage) at these volumes.
INSERT INTO billing_plans (id, name, base_fee_brl, camera_cap, trial_days) VALUES
    ('beta',     'Beta',           0.00,  2, 30),
    ('home',     'Home',          39.90,  6, NULL),
    ('business', 'Business Basic', 499.00, 16, NULL)
ON CONFLICT (id) DO NOTHING;
