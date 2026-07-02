-- Card #15: the outbound queue can now target a raw Telegram chat_id (a bot reply to the
-- specific sender), not only a paired beacon label. NULL = the existing beacon-label path
-- (target column); set = the notification consumer sends direct to that chat.
ALTER TABLE notification_queue ADD COLUMN IF NOT EXISTS chat_id bigint NULL;
