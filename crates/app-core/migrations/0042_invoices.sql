-- Customer invoices, one per (tenant, period 'YYYY-MM'). Closing a period snapshots the
-- plan (name + base fee) and the FX rate that was live, so a finalized invoice never
-- changes retroactively. `invoice_line_items` decompose the total by driver (base fee +
-- metered drivers). v1 tiers are flat, so metered lines carry amount 0 (shown for
-- transparency). Money is double precision (no numeric decoder); timestamps RFC3339 text.

CREATE TABLE IF NOT EXISTS invoices (
    id           uuid PRIMARY KEY,
    tenant_id    uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    period       text NOT NULL,                 -- 'YYYY-MM'
    plan_id      text NOT NULL,
    plan_name    text NOT NULL,
    base_fee_brl double precision NOT NULL,
    fx_usd_brl   double precision,              -- USD→BRL snapshot at close (NULL if no quote)
    total_brl    double precision NOT NULL,
    status       text NOT NULL DEFAULT 'draft', -- 'draft' | 'final'
    created_at   text NOT NULL,
    CONSTRAINT invoices_status_chk CHECK (status IN ('draft', 'final')),
    CONSTRAINT invoices_tenant_period_uq UNIQUE (tenant_id, period)
);

CREATE INDEX IF NOT EXISTS invoices_period_idx ON invoices (period, tenant_id);

CREATE TABLE IF NOT EXISTS invoice_line_items (
    id           uuid PRIMARY KEY,
    invoice_id   uuid NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    driver       text NOT NULL,               -- 'base' | 'gpu' | 'storage' | 'notifications'
    qty          double precision NOT NULL,   -- units used this period
    unit         text NOT NULL,               -- 'month' | 'gpu_sec' | 'gb' | 'count'
    included     double precision NOT NULL,   -- quota included in the plan
    billable_qty double precision NOT NULL,   -- max(0, qty - included)
    rate_brl     double precision NOT NULL,   -- per-unit overage price (BRL)
    amount_brl   double precision NOT NULL    -- billable_qty * rate_brl (base line: the fee)
);

CREATE INDEX IF NOT EXISTS invoice_line_items_invoice_idx ON invoice_line_items (invoice_id);
