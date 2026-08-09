-- Deep-link the tenant straight to the console tab that links another Tuya app account (#131).
--
-- Adding a SECOND Smart Life account to a project is a console action — there is no public API for
-- the QR (see docs/ref/tuya.md, where OAuth, password login and console automation are each ruled
-- out). So the best we can do is take the person to the exact tab instead of describing where it
-- is. That tab is a URL:
--
--   https://<region>.platform.tuya.com/cloud/basic?id=<project>&region=<dc>&toptab=related&deviceTab=4
--
-- `endpoint` already tells us the regional host (openapi.tuyaus.com → us.platform.tuya.com), but
-- the project id is NOT derivable from what we store: `access_id` is the project's Client ID, a
-- different identifier, and no Cloud API exposes the console id for it.
--
-- Both nullable, and the feature degrades rather than demands them: with no project id the button
-- opens the region's project LIST, which still beats hunting for a tab nobody knows exists.
ALTER TABLE tuya_config ADD COLUMN IF NOT EXISTS console_project_id text;
ALTER TABLE tuya_config ADD COLUMN IF NOT EXISTS console_region text;
