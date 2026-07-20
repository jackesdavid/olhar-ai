-- Reported GPU video-decode capability of an edge box (#120): the working ffmpeg hwaccel method
-- ("vaapi"/"qsv"/"cuda"/"videotoolbox"/"v4l2m2m") or "none". NULL = not yet reported (older edge).
-- Gates whether HD face capture can be enabled on this box.
ALTER TABLE tenant_boxes ADD COLUMN IF NOT EXISTS hwaccel text;
