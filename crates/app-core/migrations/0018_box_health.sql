-- Edge box health, reported on the heartbeat (headers on /config/cameras) so the Boxes
-- page can show WHY a box can't serve Live without SSHing in:
--   tailscale_state: serving | up | offline | disabled
--   ffmpeg_ok: go2rtc can produce JPEG frames (snapshots / GetCurrentFrame)
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS tailscale_state text;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS ffmpeg_ok boolean;
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS health_at timestamptz;
