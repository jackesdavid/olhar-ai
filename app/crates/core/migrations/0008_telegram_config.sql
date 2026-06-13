-- Telegram bot config → Postgres (Phase 0). A single-row table (id is pinned to 1)
-- holding the bot token + pairing code the app previously kept in telegram.json.
CREATE TABLE IF NOT EXISTS telegram_config (
    id           integer PRIMARY KEY DEFAULT 1,
    bot_token    text NOT NULL DEFAULT '',
    pairing_code text NOT NULL DEFAULT '',
    CONSTRAINT telegram_config_singleton CHECK (id = 1)
);
