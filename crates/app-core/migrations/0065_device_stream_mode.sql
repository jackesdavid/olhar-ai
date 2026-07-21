-- Per-camera stream mode (#120). NULL / 'split' = today's behaviour (substream for
-- preview/motion, main for HD/recording). 'unified' = main-only pipeline (one camera
-- session; motion on a downscaled main frame; faces cropped full-res; no substream pulled).
-- Additive + nullable, so existing rows read as split.
ALTER TABLE devices ADD COLUMN IF NOT EXISTS stream_mode text;
