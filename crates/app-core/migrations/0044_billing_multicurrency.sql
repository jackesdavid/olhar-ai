-- Multi-currency billing: sell the same tier in BRL (Brazil) and USD (American market).
-- A plan row (billing_plans) is now the *tier* — caps, trial, quotas — and the money moves
-- to `plan_prices`, one row per (plan, currency). Provider-side price objects (e.g. a
-- Stripe Price id) live in `plan_provider_price`, keeping the core provider-agnostic.
-- Invoices become currency-generic (`base_fee`/`total` + `currency`).

-- A tenant bills in exactly one currency.
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS currency text NOT NULL DEFAULT 'BRL';

CREATE TABLE IF NOT EXISTS plan_prices (
    plan_id  text NOT NULL REFERENCES billing_plans(id) ON DELETE CASCADE,
    currency text NOT NULL,
    amount   double precision NOT NULL,
    PRIMARY KEY (plan_id, currency)
);

-- Provider price refs (Stripe `price_...`). Asaas charges an arbitrary amount and has none.
CREATE TABLE IF NOT EXISTS plan_provider_price (
    plan_id   text NOT NULL REFERENCES billing_plans(id) ON DELETE CASCADE,
    provider  text NOT NULL,
    price_ref text NOT NULL,
    PRIMARY KEY (plan_id, provider)
);

-- Launch pricing, both markets.
INSERT INTO plan_prices (plan_id, currency, amount) VALUES
    ('beta',     'BRL',   0.00), ('beta',     'USD',   0.00),
    ('home',     'BRL',  39.90), ('home',     'USD',   9.99),
    ('business', 'BRL', 499.00), ('business', 'USD', 149.00)
ON CONFLICT (plan_id, currency) DO NOTHING;

-- The tier no longer carries money; prices are per-currency above.
ALTER TABLE billing_plans DROP COLUMN IF EXISTS base_fee_brl;

-- Invoices: currency-generic.
ALTER TABLE invoices RENAME COLUMN base_fee_brl TO base_fee;
ALTER TABLE invoices RENAME COLUMN total_brl TO total;
ALTER TABLE invoices ADD COLUMN IF NOT EXISTS currency text NOT NULL DEFAULT 'BRL';
ALTER TABLE invoice_line_items RENAME COLUMN rate_brl TO rate;
ALTER TABLE invoice_line_items RENAME COLUMN amount_brl TO amount;

-- Webhook idempotency: a provider event is applied exactly once (Stripe requires this;
-- it retries, and duplicate delivery must not double-apply).
CREATE TABLE IF NOT EXISTS payment_events (
    provider    text NOT NULL,
    event_id    text NOT NULL,
    received_at text NOT NULL,
    PRIMARY KEY (provider, event_id)
);
