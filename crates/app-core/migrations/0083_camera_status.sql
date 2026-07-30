-- Per-camera status, so "why is my camera not working" is answerable without a shell.
--
-- The `devices` table records what a camera IS (ip, name, model, rtsp_url) and nothing about whether
-- any of it WORKS. On 2026-07-30 diagnosing one dead camera took `ping` and `nc` on the edge box:
-- the UI showed `has_stream: true` while the camera sat unplugged, and the frame endpoint answered
-- 502 "the camera's decoder produced nothing" — a symptom three layers below the cause, naming
-- neither the stage that failed nor the fix.
--
-- The diagnosis already existed. `app_core::rtsp_probe::ProbeStatus` distinguishes unreachable /
-- timeout / auth_failed / locked / not_found / no_credentials / not_on_this_box / streaming — it was
-- computed once by the credential wizard and thrown away. These columns are where it lands.
--
-- `status` is the ProbeStatus snake_case serialization, stored as text rather than a Postgres enum:
-- the set is defined in Rust and adding a variant there must not require a migration lockstep.
ALTER TABLE devices ADD COLUMN IF NOT EXISTS probe_status text;
-- The camera's OWN words where we have them ("401 Unauthorized", "Can't reach 192.168.0.7: …").
-- The reason `detail` exists at all: a status tells the user which stage failed, the detail is what
-- lets them or us tell WHY within that stage.
ALTER TABLE devices ADD COLUMN IF NOT EXISTS probe_detail text;
-- RFC3339 (house rule). When the status was last established — a status with no age is a status
-- nobody can trust, and "last checked 4 days ago" is itself the useful answer.
ALTER TABLE devices ADD COLUMN IF NOT EXISTS probe_at text;
-- RFC3339. Last time a frame actually arrived. Distinct from `probe_status = ok`, which only means
-- the stream could be DESCRIBED: a camera can pass the probe and still deliver nothing (wedged
-- encoder, pipeline that never negotiated). This is what separates "Streaming" from "Reachable".
ALTER TABLE devices ADD COLUMN IF NOT EXISTS last_frame_at text;

COMMENT ON COLUMN devices.probe_status IS
  'app_core::rtsp_probe::ProbeStatus, snake_case. NULL = never checked.';
