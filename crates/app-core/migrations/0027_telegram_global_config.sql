-- Single-row global Telegram provider config (operator-set, shared by all tenants).
-- Enforced as a single row by the CHECK constraint.
CREATE TABLE IF NOT EXISTS telegram_global_config (
    id           integer PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    bot_token    text NOT NULL DEFAULT '',
    bot_username text
);
INSERT INTO telegram_global_config (id, bot_token) VALUES (1, '') ON CONFLICT DO NOTHING;
