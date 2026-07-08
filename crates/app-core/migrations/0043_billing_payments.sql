-- Payment-provider linkage. `provider` ('asaas' now; 'stripe' later for international) is
-- the seam that keeps the billing core provider-agnostic — only charge creation + webhook
-- handling are provider-specific. Asaas owns the recurring cycle + dunning; we mirror its
-- customer/subscription refs and react to webhooks to gate access.

CREATE TABLE IF NOT EXISTS payment_customers (
    tenant_id    uuid PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    provider     text NOT NULL,
    customer_ref text NOT NULL,      -- provider's customer id
    created_at   text NOT NULL
);

CREATE TABLE IF NOT EXISTS payment_subscriptions (
    id               uuid PRIMARY KEY,
    tenant_id        uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    provider         text NOT NULL,
    subscription_ref text NOT NULL,   -- provider's subscription id
    plan_id          text NOT NULL,
    billing_type     text NOT NULL,   -- 'PIX' | 'BOLETO' | 'CREDIT_CARD'
    status           text NOT NULL,   -- 'active' | 'past_due' | 'canceled'
    created_at       text NOT NULL,
    updated_at       text NOT NULL
);

-- Webhook lookup: map an incoming (provider, subscription_ref) back to its tenant.
CREATE INDEX IF NOT EXISTS payment_subscriptions_ref_idx
    ON payment_subscriptions (provider, subscription_ref);
CREATE INDEX IF NOT EXISTS payment_subscriptions_tenant_idx
    ON payment_subscriptions (tenant_id);
