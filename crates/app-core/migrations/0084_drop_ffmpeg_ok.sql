-- The media path is GStreamer, in-process. The box health signal that tracked whether a second,
-- external decoder happened to be on PATH no longer describes anything the product does, and a
-- column that reports on a tool nothing runs can only mislead whoever reads it next.
--
-- `gstreamer_ok` remains and is the whole answer.
ALTER TABLE tenant_boxes DROP COLUMN IF EXISTS ffmpeg_ok;
