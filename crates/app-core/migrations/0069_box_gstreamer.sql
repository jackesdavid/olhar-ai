-- Reported GStreamer presence of an edge box (card #138, GStreamer media backend option):
-- whether `gst-launch-1.0` is on PATH. NULL = not yet reported (older edge). Mirrors `ffmpeg_ok`.
-- Lets an operator tell whether a box is actually ready before flipping its `media_backend` to
-- "gstreamer" (recording_config jsonb — no column needed for the flag itself).
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS gstreamer_ok boolean;
