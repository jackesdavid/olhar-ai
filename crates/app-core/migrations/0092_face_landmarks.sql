-- Persist a face's landmarks instead of recomputing them on every view.
--
-- The detail overlay called the AI sidecar on EVERY open: fetch the crop, run `face_landmarks`
-- (InsightFace `landmark_2d_106`), draw. One inference per view, for points that never change —
-- a face is stored once and looked at many times, so the cost was on the wrong side of that ratio.
-- It also made the overlay fail whenever the sidecar was down, which reads as "this face has no
-- landmarks" rather than "we could not ask".
--
-- Storing them costs ~106 pairs of numbers per face and makes the view free and offline-safe.
--
-- jsonb, not a float array: the points are `[[x, y], …]` and are read back straight into JSON for
-- the client. A flat `real[]` would save a little space and require every reader to re-pair them.
ALTER TABLE faces ADD COLUMN IF NOT EXISTS landmarks jsonb;

-- Which detector produced them, because two different sets exist and they are NOT interchangeable:
--   'insightface_106' — dense, from the sidecar, what the detail overlay draws
--   'yunet_5'         — the edge's 5 keypoints, what ArcFace alignment and the #126 pose estimate
--                       are computed from
-- A view that assumes 106 and receives 5 would draw five dots and look broken; a pose recomputed
-- from the dense set would not match the stored yaw/pitch/roll. Recording the source keeps the two
-- straight instead of leaving a reader to guess from the array length.
ALTER TABLE faces ADD COLUMN IF NOT EXISTS landmarks_source text;
