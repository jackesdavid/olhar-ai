-- Foreign-exchange rates, append-only. Our costs (Contabo, RunPod) are USD; BRL is the
-- billing/display currency. A daily fetch inserts one row; if the fetch fails, nothing is
-- inserted and readers fall back to the latest existing row (so there is always a quote).
-- Append-only so a closed invoice can pin the rate that was live at close (auditable).
CREATE TABLE IF NOT EXISTS fx_rates (
    id         uuid PRIMARY KEY,
    base       text NOT NULL,               -- e.g. 'USD'
    quote      text NOT NULL,               -- e.g. 'BRL'
    rate       double precision NOT NULL,   -- 1 base = `rate` quote
    source     text NOT NULL,               -- provider id, for provenance
    fetched_at text NOT NULL                -- RFC3339
);

CREATE INDEX IF NOT EXISTS fx_rates_pair_idx ON fx_rates (base, quote, fetched_at DESC);
