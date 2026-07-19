-- Pre-cropped face thumbnail key. The stored `image_key` is the whole FRAME (needed for the
-- Context / full-frame+box view); `crop_key` is a small face crop generated once at StoreFace time
-- so the image proxy serves thumbnails as a pure passthrough (no per-request decode/crop). NULL for
-- rows captured before this (the proxy falls back to cropping the frame on the fly).
ALTER TABLE faces ADD COLUMN IF NOT EXISTS crop_key text;
