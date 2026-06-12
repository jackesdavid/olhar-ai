-- Notification targets (beacons) → Postgres (Phase 0).
-- A trusted Telegram destination, keyed by chat_id (unique per chat). The in-memory
-- NotificationStore stays the working set; this is the persistence target. Replace-all
-- on each mutation mirrors the devices table.
CREATE TABLE IF NOT EXISTS notification_targets (
    chat_id  bigint PRIMARY KEY,            -- Telegram chat id
    label    text    NOT NULL DEFAULT '',   -- human label (sender name at pairing)
    enabled  boolean NOT NULL DEFAULT true  -- receives alerts?
);
