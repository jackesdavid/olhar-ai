-- A provider price object belongs to one currency (card #63).
--
-- `plan_provider_price` was keyed `(plan_id, provider)`, but the Stripe Price it points at is
-- created with a single currency — the currency of whichever tenant subscribed to that tier
-- first. Every later subscriber to the same tier reused that Price. So a Brazilian signing up
-- for `home` first minted a BRL R$ 39,90 Price, and the next American was charged **R$ 39,90**
-- instead of US$ 9,99; reverse the order and Brazilians pay US$ 9,99 (≈ R$ 54). Whoever arrived
-- first silently decided the currency and the amount for everyone else on that plan.
--
-- `plan_prices` has always been keyed by currency. This makes its provider-side mirror agree.
--
-- Existing rows carry no currency and there is no way to infer one — the Price lives in the
-- provider, and guessing wrong is exactly the bug. They are dropped rather than backfilled;
-- `start_subscription` mints a fresh Price on the next subscribe and persists it correctly.
-- (Production has none: this ships before the first live charge.)

DELETE FROM plan_provider_price;

ALTER TABLE plan_provider_price ADD COLUMN IF NOT EXISTS currency text NOT NULL;

ALTER TABLE plan_provider_price DROP CONSTRAINT IF EXISTS plan_provider_price_pkey;
ALTER TABLE plan_provider_price ADD PRIMARY KEY (plan_id, provider, currency);
