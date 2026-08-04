-- #126: persist the face quality signals alongside the capture.
--
-- Not needed to GATE — the gate is composed in the graph from the `face_detected` ports and decides
-- before anything is stored. These columns exist so the Faces workspace can show WHY a capture was
-- kept, and so threshold calibration (#128) has real measurements to fit against instead of guesses.
--
-- NULLABLE on purpose, unlike `confidence`/`face_width` which default to 0. For a quality signal
-- "not measured" and "measured as zero" are different facts: a box older than #126 reports nothing,
-- and 0 is a legitimate blur score for a completely flat crop. Storing the unknown case as 0 would
-- quietly poison any calibration that averaged these columns. `StoreFace` therefore writes NULL
-- when it sees the fail-open sentinel (`face_quality::BLUR_UNKNOWN`) rather than the sentinel value.
--
-- `real` matches `confidence`; these are gate signals, not money.
ALTER TABLE faces ADD COLUMN IF NOT EXISTS blur  real;
ALTER TABLE faces ADD COLUMN IF NOT EXISTS yaw   real;
ALTER TABLE faces ADD COLUMN IF NOT EXISTS pitch real;
ALTER TABLE faces ADD COLUMN IF NOT EXISTS roll  real;
