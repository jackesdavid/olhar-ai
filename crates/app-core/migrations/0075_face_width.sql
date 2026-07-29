-- #117: replace the `face_ratio` face-size signal with `face_width` (box width in source pixels).
--
-- `face_ratio` was the face's area as a percentage of the frame (0..100), but it rode `PortType::Num`
-- — an i64 — so control rounded it to an integer before the graph ever saw it. A face has to cover
-- ~0.5% of the frame to round to 1 (about 102x102 px in 1080p), so in practice every real detection
-- reported 0 and any `NumGte` gate on it was dead. Pixel width is an integer by nature, survives the
-- port type intact, and is the quantity recognition quality actually tracks (ArcFace aligns to
-- 112x112, so a narrower face is upscaled and loses detail).
--
-- The stored percentages cannot be converted: recovering a width needs the frame dimensions and the
-- box aspect, neither of which is on the row. They are dropped rather than carried forward as a
-- number that would silently mean something else. Existing rows get 0 = "unknown", which is what
-- they effectively already held.
--
-- Idempotent so a retried boot can't wedge the migration runner.
ALTER TABLE faces DROP COLUMN IF EXISTS face_ratio;
ALTER TABLE faces ADD COLUMN IF NOT EXISTS face_width integer NOT NULL DEFAULT 0;
