-- Reap clips stuck "in transit" (card #64).
--
-- Control flips a clip to `uploading` the moment it mints the presigned PUT, and only the edge
-- can flip it forward. Nothing flips it back. So every way an upload can die — the box loses
-- power mid-PUT, the upload queue is full, the edge gives up after its retries, the confirm POST
-- never lands — leaves a row reading `uploading` forever, and the Recordings page shows an
-- "In transit" badge for a clip that is never coming.
--
-- The ticket expires after `UPLOAD_URL_TTL_SECS` (1 h, sentinel-control/src/http.rs). Past that
-- the edge *cannot* succeed, whatever it is doing, so the state is knowably wrong. `created_at`
-- is written when the manifest arrives, microseconds before the presign, so it dates the ticket.
-- Two hours leaves a wide margin for clock skew and a slow final retry.
--
-- Deliberately does NOT clear `cloud_key`: if the bytes did land and only the confirm was lost,
-- the key is how we find the orphan. R2's lifecycle expires it either way.

CREATE INDEX IF NOT EXISTS recordings_uploading_idx
    ON recordings (created_at) WHERE upload_state = 'uploading';

SELECT cron.schedule(
  'stuck-upload-sweep',
  '17 * * * *',
  $cron$
    UPDATE recordings
       SET upload_state = 'failed'
     WHERE upload_state = 'uploading'
       AND created_at::timestamptz < now() - interval '2 hours'
  $cron$
);
